//! Bridge from CLI config/flags to the spqx-core inference engine: model and
//! reference resolution plus streaming ICL synthesis.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use spqx_core::audio::resample;
use spqx_core::synthesis::{Sampling, Session, Stop};

use crate::config::Config;
use crate::paths;

/// A resolved reference voice (audio + transcript) for ICL cloning.
pub struct Reference {
    pub wav: PathBuf,
    pub text: String,
    pub label: String,
    pub frozen: Option<spqx_core::conditioning::FrozenConditioning>,
    pub seed: Option<u64>,
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
            frozen: None,
            seed: None,
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
        frozen: None,
        seed: None,
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
    pub backend: &'static str,
    pub ttfa_s: f64,
    pub wall_s: f64,
    pub audio_s: f64,
    pub sample_rate: u32,
    /// False when generation hit the frame cap instead of emitting EOS, which
    /// means the utterance is cut off mid-sentence.
    pub eos_reached: bool,
}

/// Load the model, build an ICL session for `reference`, and stream synthesis
/// of `text`. `on_chunk` receives f32 PCM as it is produced; the first chunk
/// has already had leading-non-speech gating applied.
pub fn synthesize_streaming(
    config: &Config,
    reference: &Reference,
    text: &str,
    on_chunk: impl FnMut(&[f32]) + Send,
) -> Result<SynthStats> {
    // Keep the engine's println! progress off the CLI's stdout so `--json`
    // and piped output stay clean; it still reaches a human via stderr.
    let _stdout_guard = spqx_core::stdio::StdoutGuard::new()?;

    let model_dir = resolve_model_dir(config)?;
    let language = spqx_core::worker::normalize_language(&config.voice.language);
    let session = if let Some(frozen) = &reference.frozen {
        Session::load_frozen(
            &model_dir,
            frozen,
            &language,
            reference.seed,
            &config.engine,
        )?
    } else {
        Session::load(
            &model_dir,
            &reference.wav,
            &reference.text,
            &language,
            &config.engine,
        )?
    };

    stream_session(config, &session, text, reference.frozen.is_none(), on_chunk)
}

/// Native description-only generation; never resolves or substitutes a registry reference.
/// Returns generation statistics and preparation time separately. No audio postprocessing.
pub fn design_streaming(
    config: &Config,
    description: &str,
    seed: Option<u64>,
    text: &str,
    on_chunk: impl FnMut(&[f32]) + Send,
) -> Result<(SynthStats, f64)> {
    let _stdout_guard = spqx_core::stdio::StdoutGuard::new()?;
    let started = std::time::Instant::now();
    let model = resolve_model_dir(config)?;
    let session = Session::load_voice_design(
        &model,
        description,
        &spqx_core::worker::normalize_language(&config.voice.language),
        seed,
        &config.engine,
    )?;
    let prepare_s = started.elapsed().as_secs_f64();
    Ok((
        stream_session(config, &session, text, false, on_chunk)?,
        prepare_s,
    ))
}

fn stream_session(
    config: &Config,
    session: &Session,
    text: &str,
    gate_leading: bool,
    mut on_chunk: impl FnMut(&[f32]) + Send,
) -> Result<SynthStats> {
    let sample_rate = config.audio.sample_rate_hz;
    let started = std::time::Instant::now();
    let mut ttfa_s = 0.0;
    let mut first = true;
    let mut audio_samples = 0usize;

    let sampling = Sampling {
        temperature: config.sampling.temperature,
        top_k: config.sampling.top_k,
        max_new_tokens: 1536,
        chunk_size: 4,
    };
    let mut stream_error = None;
    let stop = session.synthesize(
        text,
        &sampling,
        |chunk, chunk_sr| {
            let mut buffer = if chunk_sr == sample_rate {
                chunk.to_vec()
            } else {
                match resample(chunk, chunk_sr, sample_rate) {
                    Ok(samples) => samples,
                    Err(error) => {
                        stream_error = Some(error);
                        return false;
                    }
                }
            };
            if first {
                first = false;
                ttfa_s = started.elapsed().as_secs_f64();
                if gate_leading {
                    spqx_core::postprocess::gate_leading_nonspeech(&mut buffer, sample_rate);
                }
            }
            audio_samples += buffer.len();
            on_chunk(&buffer);
            true
        },
        || false,
    )?;
    if let Some(error) = stream_error {
        return Err(error.into());
    }

    Ok(SynthStats {
        backend: session.backend_name(),
        ttfa_s,
        wall_s: started.elapsed().as_secs_f64(),
        audio_s: audio_samples as f64 / sample_rate as f64,
        sample_rate,
        eos_reached: stop == Stop::Eos,
    })
}

fn expand(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(path)
}

/// Newest snapshot directory for an HF model id in the local cache.
fn hf_snapshot(model_id: &str) -> Option<PathBuf> {
    let cache = if let Some(cache) = std::env::var_os("HF_HUB_CACHE") {
        PathBuf::from(cache)
    } else if let Some(home) = std::env::var_os("HF_HOME") {
        PathBuf::from(home).join("hub")
    } else {
        PathBuf::from(std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?)
            .join(".cache/huggingface/hub")
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
