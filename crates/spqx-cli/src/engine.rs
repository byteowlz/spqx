//! Bridge from CLI config/flags to the spqx-core inference engine: model and
//! reference resolution plus streaming ICL synthesis.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use spqx_core::audio::{load_wav_file, resample};
use spqx_core::audio_encoder::AudioEncoder;
use spqx_core::inference::TTSInference;
use spqx_core::speaker_encoder::SpeakerEncoder;
use spqx_core::tensor::Device;

use crate::config::Config;
use crate::paths;

extern "C" {
    fn dup(fd: i32) -> i32;
    fn dup2(oldfd: i32, newfd: i32) -> i32;
    fn close(fd: i32) -> i32;
}

/// Redirects the process's stdout (fd 1) to stderr (fd 2) for its lifetime,
/// restoring it on drop. The engine prints progress with `println!`; this
/// keeps the CLI's real stdout clean for `--json` while that chatter still
/// reaches a human on stderr.
struct StdoutToStderr {
    saved: i32,
}

impl StdoutToStderr {
    fn new() -> Option<Self> {
        unsafe {
            let saved = dup(1);
            if saved < 0 {
                return None;
            }
            if dup2(2, 1) < 0 {
                close(saved);
                return None;
            }
            Some(Self { saved })
        }
    }
}

impl Drop for StdoutToStderr {
    fn drop(&mut self) {
        use std::io::Write;
        let _ = std::io::stdout().flush();
        unsafe {
            dup2(self.saved, 1);
            close(self.saved);
        }
    }
}

/// A resolved reference voice (audio + transcript) for ICL cloning.
pub struct Reference {
    pub wav: PathBuf,
    pub text: String,
    pub label: String,
}

/// Resolve the local model directory: explicit `model.path`, else the newest
/// Hugging Face cache snapshot of `model.id`.
pub fn resolve_model_dir(config: &Config) -> Result<PathBuf> {
    if let Some(path) = &config.model.path {
        let path = expand(path);
        if path.exists() {
            return Ok(path);
        }
        bail!("model.path {} does not exist", path.display());
    }
    hf_snapshot(&config.model.id).with_context(|| {
        format!(
            "model {} not found in the Hugging Face cache; download it or set model.path",
            config.model.id
        )
    })
}

/// Resolve a reference voice from explicit paths, else a registry voice name,
/// else the config default voice.
pub fn resolve_reference(
    config: &Config,
    voice: Option<&str>,
    ref_audio: Option<&Path>,
    ref_text: Option<&str>,
    ref_text_file: Option<&Path>,
) -> Result<Reference> {
    if let Some(wav) = ref_audio {
        let text = if let Some(text) = ref_text {
            text.to_string()
        } else if let Some(file) = ref_text_file {
            std::fs::read_to_string(file)
                .with_context(|| format!("reading ref text file {}", file.display()))?
                .trim()
                .to_string()
        } else {
            bail!("--ref-audio requires --ref-text or --ref-text-file");
        };
        return Ok(Reference {
            wav: wav.to_path_buf(),
            text,
            label: wav.display().to_string(),
        });
    }

    let name = voice.unwrap_or(&config.voice.default);
    let dir = voices_dir(config)?.join(name);
    let wav = dir.join("reference.wav");
    let txt = dir.join("reference.txt");
    if !wav.exists() || !txt.exists() {
        bail!(
            "voice '{name}' not found in the registry ({}). Add one with \
             `spqx voices add {name} --ref-audio <wav> --ref-text-file <txt>`, \
             or pass --ref-audio/--ref-text directly.",
            dir.display()
        );
    }
    let text = std::fs::read_to_string(&txt)
        .with_context(|| format!("reading {}", txt.display()))?
        .trim()
        .to_string();
    Ok(Reference {
        wav,
        text,
        label: name.to_string(),
    })
}

pub fn voices_dir(config: &Config) -> Result<PathBuf> {
    match &config.paths.voices_dir {
        Some(dir) => Ok(expand(dir)),
        None => paths::voices_dir(),
    }
}

/// Timing/summary of one synthesis run.
pub struct SynthStats {
    pub ttfa_s: f64,
    pub wall_s: f64,
    pub audio_s: f64,
    pub sample_rate: u32,
}

/// Load the model, build an ICL session for `reference`, and stream synthesis
/// of `text`. `on_chunk` receives f32 PCM as it is produced; the first chunk
/// has already had leading-non-speech gating applied.
pub fn synthesize_streaming(
    config: &Config,
    reference: &Reference,
    text: &str,
    mut on_chunk: impl FnMut(&[f32]),
) -> Result<SynthStats> {
    // Keep the engine's println! progress off the CLI's stdout so `--json`
    // and piped output stay clean; it still reaches a human via stderr.
    let _stdout_guard = StdoutToStderr::new();

    #[cfg(feature = "mlx")]
    spqx_core::backend::mlx::stream::init_mlx(true);

    let device = Device::Cpu;
    let model_dir = resolve_model_dir(config)?;
    let inference = TTSInference::new(&model_dir, device)?;

    let speaker_encoder = SpeakerEncoder::load(
        inference.weights(),
        &inference.config().speaker_encoder_config,
        device,
    )?;
    let tokenizer_path = model_dir.join("speech_tokenizer").join("model.safetensors");
    let audio_encoder = AudioEncoder::load(&tokenizer_path, device)?;

    let se_sr = inference.config().speaker_encoder_config.sample_rate;
    let (samples, sr) = load_wav_file(
        reference
            .wav
            .to_str()
            .context("reference path is not valid UTF-8")?,
    )?;
    let samples = if sr == se_sr {
        samples
    } else {
        resample(&samples, sr, se_sr)?
    };
    let speaker_embedding = speaker_encoder.extract_embedding(&samples)?;
    let ref_codes = audio_encoder.encode(&samples)?;

    let session = inference.prepare_icl_session(
        &reference.text,
        &ref_codes,
        &speaker_embedding,
        &config.voice.language,
    )?;

    let sample_rate = config.audio.sample_rate_hz;
    let started = std::time::Instant::now();
    let mut ttfa_s = 0.0;
    let mut first = true;
    let mut audio_samples = 0usize;

    inference.generate_with_icl_session_streaming(
        &session,
        text,
        config.sampling.temperature,
        config.sampling.top_k,
        i64::MAX,
        4,
        |chunk, chunk_sr| {
            let mut buffer = if chunk_sr == sample_rate {
                chunk.to_vec()
            } else {
                resample(chunk, chunk_sr, sample_rate).unwrap_or_else(|_| chunk.to_vec())
            };
            if first {
                first = false;
                ttfa_s = started.elapsed().as_secs_f64();
                spqx_core::postprocess::gate_leading_nonspeech(&mut buffer, sample_rate);
            }
            audio_samples += buffer.len();
            on_chunk(&buffer);
            true
        },
    )?;

    Ok(SynthStats {
        ttfa_s,
        wall_s: started.elapsed().as_secs_f64(),
        audio_s: audio_samples as f64 / sample_rate as f64,
        sample_rate,
    })
}

fn expand(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(path)
}

/// Newest snapshot directory for an HF model id in the local cache.
fn hf_snapshot(model_id: &str) -> Option<PathBuf> {
    let cache = match std::env::var_os("HF_HOME") {
        Some(hf) => PathBuf::from(hf).join("hub"),
        None => PathBuf::from(std::env::var_os("HOME")?).join(".cache/huggingface/hub"),
    };
    let snapshots = cache
        .join(format!("models--{}", model_id.replace('/', "--")))
        .join("snapshots");
    let mut entries: Vec<PathBuf> = std::fs::read_dir(&snapshots)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    entries.sort();
    entries.pop()
}
