//! `spqx say` — synthesize text with live playback and/or WAV output.

use std::io::Read;
use std::path::PathBuf;
use std::sync::mpsc;

use anyhow::{bail, Context, Result};
use clap::Args;

use crate::config::Config;
use crate::engine::{self, Reference};
use crate::playback::Player;
use crate::CommonOpts;

#[derive(Debug, Args)]
pub struct SayArgs {
    /// Text to speak. Omit or use "-" to read from stdin.
    pub text: Option<String>,
    /// Voice: a registry name (see `spqx voices`) or the config default.
    #[arg(long)]
    #[arg(conflicts_with = "voice_id")]
    pub voice: Option<String>,
    /// Canonical voqx voice identity; never joins by display name.
    #[arg(long, conflicts_with_all = ["voice", "ref_audio", "ref_text", "ref_text_file"])]
    pub voice_id: Option<String>,
    /// Explicit performance within the canonical synthesis profile.
    #[arg(long, requires = "voice_id")]
    pub reference_id: Option<String>,
    /// Override the canonical registry root (otherwise XDG voice-registry).
    #[arg(long, requires = "voice_id")]
    pub registry: Option<PathBuf>,
    /// Explicit synthesis language (separate from the archived reference language).
    #[arg(long)]
    pub language: Option<String>,
    /// Override a saved frozen voice's sampler seed.
    #[arg(long, requires = "voice_id")]
    pub seed: Option<u64>,
    /// Reference WAV for ad-hoc voice cloning (bypasses the registry).
    #[arg(long, value_name = "WAV")]
    pub ref_audio: Option<PathBuf>,
    /// Transcript of --ref-audio.
    #[arg(long, value_name = "TEXT")]
    pub ref_text: Option<String>,
    /// File containing the transcript of --ref-audio.
    #[arg(long, value_name = "PATH")]
    pub ref_text_file: Option<PathBuf>,
    /// Write synthesized audio to this WAV file.
    #[arg(long, value_name = "PATH")]
    pub out: Option<PathBuf>,
    /// Do not play audio live (implied when only --out is given with --quiet).
    #[arg(long)]
    pub no_play: bool,
}

impl SayArgs {
    fn effective_config(&self, common: &CommonOpts) -> Result<Config> {
        let mut config = common.load_config()?;
        if let Some(language) = &self.language {
            config.voice.language = language.clone();
        }
        Ok(config)
    }
    fn reference(&self, config: &Config) -> Result<Reference> {
        let mut reference = if let Some(id) = &self.voice_id {
            crate::bundles::resolve(id, self.reference_id.as_deref(), self.registry.as_deref())?
        } else {
            engine::resolve_reference(
                config,
                self.voice.as_deref(),
                self.ref_audio.as_deref(),
                self.ref_text.as_deref(),
                self.ref_text_file.as_deref(),
            )?
        };
        if self.seed.is_some() {
            reference.seed = self.seed;
        }
        Ok(reference)
    }
}
pub fn run(args: SayArgs, common: &CommonOpts) -> Result<()> {
    let config = args.effective_config(common)?;

    let text = match args.text.as_deref() {
        Some("-") | None => read_stdin()?,
        Some(text) => text.to_string(),
    };
    if text.trim().is_empty() {
        bail!("no text to speak (pass TEXT or pipe it on stdin)");
    }

    let reference = args.reference(&config)?;

    let play = config.audio.play && !args.no_play;
    let want_wav = args.out.is_some();

    // Playback runs on rodio's own thread, fed chunk-by-chunk over a channel.
    let (tx, player) = if play {
        let (tx, rx) = mpsc::channel::<Vec<f32>>();
        let player =
            Player::start(rx, config.audio.sample_rate_hz).context("starting audio playback")?;
        (Some(tx), Some(player))
    } else {
        (None, None)
    };

    let mut wav_samples: Vec<f32> = Vec::new();
    let stats = engine::synthesize_streaming(&config, &reference, &text, |chunk| {
        if want_wav {
            wav_samples.extend_from_slice(chunk);
        }
        if let Some(tx) = &tx {
            let _ = tx.send(chunk.to_vec());
        }
    })?;

    drop(tx); // signal end of stream to the player
    if let Some(player) = player {
        player.wait();
    }

    // A collapsed generation stops speaking but keeps emitting frames, so the
    // tail is silence the user never asked for. Drop it before writing.
    let trailing_s =
        spqx_core::postprocess::trailing_nonspeech_seconds(&wav_samples, stats.sample_rate);
    spqx_core::postprocess::trim_trailing_nonspeech(&mut wav_samples, stats.sample_rate, 0.5);

    let wav_path = if let Some(out) = &args.out {
        let out = resolve_out(&config, out);
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        spqx_core::audio::write_wav_file(
            out.to_str().context("output path is not valid UTF-8")?,
            &wav_samples,
            stats.sample_rate,
        )?;
        Some(out)
    } else {
        None
    };

    report(common, &reference, &stats, wav_path.as_deref());

    // Reported after the WAV is written so the partial audio is still
    // inspectable, but the exit code must not claim success.
    if !stats.eos_reached {
        bail!(
            "generation hit the frame cap without reaching EOS: the utterance is truncated. \
             Split the text into shorter segments."
        );
    }
    if trailing_s > MAX_TRAILING_S {
        bail!(
            "generation collapsed after {:.0}s: {trailing_s:.0}s of trailing non-speech. \
             Split the text into shorter segments.",
            stats.audio_s - trailing_s,
        );
    }
    Ok(())
}

/// Longer than a natural utterance release, shorter than any observed
/// collapse (the shortest measured was 13s of trailing silence).
const MAX_TRAILING_S: f64 = 3.0;

fn report(
    common: &CommonOpts,
    reference: &Reference,
    stats: &engine::SynthStats,
    wav_path: Option<&std::path::Path>,
) {
    let rtf = if stats.audio_s > 0.0 {
        stats.wall_s / stats.audio_s
    } else {
        0.0
    };
    if common.json {
        let obj = serde_json::json!({
            "voice": reference.label,
            "ttfa_ms": (stats.ttfa_s * 1000.0).round(),
            "audio_s": (stats.audio_s * 1000.0).round() / 1000.0,
            "wall_s": (stats.wall_s * 1000.0).round() / 1000.0,
            "rtf": (rtf * 1000.0).round() / 1000.0,
            "sample_rate": stats.sample_rate,
            "eos_reached": stats.eos_reached,
            "wav": wav_path.map(|p| p.display().to_string()),
        });
        println!("{}", serde_json::to_string(&obj).unwrap_or_default());
    } else if !common.quiet {
        eprintln!(
            "voice={} ttfa={:.0}ms audio={:.1}s rtf={:.2}{}",
            reference.label,
            stats.ttfa_s * 1000.0,
            stats.audio_s,
            rtf,
            wav_path
                .map(|p| format!(" -> {}", p.display()))
                .unwrap_or_default(),
        );
    }
}

pub(crate) fn resolve_out(config: &Config, out: &std::path::Path) -> PathBuf {
    if out.is_absolute() || out.components().count() > 1 {
        return out.to_path_buf();
    }
    match &config.audio.output_dir {
        Some(dir) => PathBuf::from(dir).join(out),
        None => out.to_path_buf(),
    }
}

fn read_stdin() -> Result<String> {
    let mut buf = String::new();
    std::io::stdin()
        .read_to_string(&mut buf)
        .context("reading text from stdin")?;
    Ok(buf)
}
