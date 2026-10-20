//! Description-only native voice candidates. No registry/reference fallback or enrollment.
use std::{io::Write, path::PathBuf};

use anyhow::{bail, Context, Result};
use clap::Args;

use crate::{engine, CommonOpts};

#[derive(Debug, Args)]
pub struct DesignArgs {
    /// Text spoken by the newly described voice (not the description).
    pub text: String,
    /// Describe a new voice; requires an actual VoiceDesign checkpoint.
    #[arg(long, value_name = "DESCRIPTION")]
    pub instruct: String,
    /// Language name/code; otherwise uses voice.language from configuration.
    #[arg(long)]
    pub language: Option<String>,
    /// Explicit native sampler seed. Unset means random; not a portable voice identity.
    #[arg(long)]
    pub seed: Option<u64>,
    /// Save the completed candidate WAV. Existing files are not overwritten. No live playback.
    #[arg(long, value_name = "PATH")]
    pub out: PathBuf,
}

pub fn run(args: DesignArgs, common: &CommonOpts) -> Result<()> {
    if args.text.trim().is_empty() {
        bail!("VoiceDesign requires nonempty spoken text");
    }
    let mut config = common.load_config()?;
    if let Some(language) = &args.language {
        config.voice.language = language.clone();
    }
    if config.audio.sample_rate_hz != 24000 {
        bail!("VoiceDesign candidates require native 24000 Hz output; set audio.sample_rate_hz = 24000");
    }
    let out = crate::say::resolve_out(&config, &args.out);
    if out.exists() {
        bail!("candidate output already exists: {}", out.display());
    }
    let started = std::time::Instant::now();
    let mut samples = Vec::new();
    let (stats, prepare_s) =
        engine::design_streaming(&config, &args.instruct, args.seed, &args.text, |chunk| {
            samples.extend_from_slice(chunk)
        })?;
    validate_candidate(&samples, stats.eos_reached)?;
    // Candidate audio is not trimmed, denoised or regenerated after selection.
    if let Some(parent) = out.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    let bytes = spqx_core::audio::write_wav_bytes(&samples, stats.sample_rate)?;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&out)
        .with_context(|| format!("creating candidate without overwriting {}", out.display()))?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    if common.json {
        println!(
            "{}",
            serde_json::json!({
                "operation": "voice_design", "backend": stats.backend,
                "model": engine::resolve_model_dir(&config)?,
                "text": args.text, "description": args.instruct,
                "language": spqx_core::worker::normalize_language(&config.voice.language),
                "seed": args.seed, "sample_rate": stats.sample_rate,
                "prepare_ms": prepare_s * 1000.0,
                "first_pcm_ms_after_prepare": stats.ttfa_s * 1000.0,
                "generation_wall_s": stats.wall_s,
                "total_wall_s": started.elapsed().as_secs_f64(), "audio_s": stats.audio_s,
                "eos_reached": stats.eos_reached, "wav": out,
                "canonical_voice_id": null, "registry_promoted": false,
            })
        );
    } else if !common.quiet {
        eprintln!(
            "VoiceDesign candidate -> {} ({:.2}s audio; no registry enrollment)",
            out.display(),
            stats.audio_s
        );
    }
    Ok(())
}

fn validate_candidate(samples: &[f32], eos: bool) -> Result<()> {
    if !eos {
        bail!("VoiceDesign reached the frame cap without EOS; candidate not saved");
    }
    if samples.is_empty()
        || samples.iter().any(|v| !v.is_finite() || v.abs() > 1.0)
        || !samples.iter().any(|v| v.abs() > 1e-6)
    {
        bail!("VoiceDesign returned empty, nonfinite, out-of-range or silent audio; candidate not saved");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn candidate_success_requires_complete_finite_non_silent_audio() {
        for samples in [
            vec![],
            vec![0.0; 10],
            vec![f32::NAN],
            vec![f32::INFINITY],
            vec![1.1],
        ] {
            assert!(validate_candidate(&samples, true).is_err());
        }
        assert!(validate_candidate(&[0.1, -0.1], false).is_err());
        assert!(validate_candidate(&[0.1, -0.1], true).is_ok());
    }
}
