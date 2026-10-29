//! Description-only native voice candidates. No registry/reference fallback or enrollment.
use std::{io::Write, path::PathBuf, sync::mpsc, time::Instant};

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
    /// Save the completed candidate WAV without overwriting an existing file.
    #[arg(long, value_name = "PATH")]
    pub out: Option<PathBuf>,
    /// Do not play audio live; requires --out when playback is disabled.
    #[arg(long)]
    pub no_play: bool,
}

/// One accepted candidate plus the seeds rejected on the way to it.
struct Candidate {
    seed: u64,
    rejected: Vec<u64>,
    samples: Vec<f32>,
    stats: engine::SynthStats,
    prepare_s: f64,
    model: PathBuf,
    language: String,
}

pub fn run(args: DesignArgs, common: &CommonOpts) -> Result<()> {
    let started = Instant::now();
    let config = validate(&args, common)?;
    let out = args
        .out
        .as_ref()
        .map(|path| crate::say::resolve_out(&config, path));
    if let Some(path) = &out {
        if path.exists() {
            bail!("candidate output already exists: {}", path.display());
        }
    }
    let play = config.audio.play && !args.no_play;
    if !play && out.is_none() {
        bail!("playback is disabled; specify --out to save the candidate");
    }
    let (tx, player) = if play {
        let (tx, rx) = mpsc::channel();
        let player = crate::playback::Player::start(rx, config.audio.sample_rate_hz)
            .context("starting audio playback")?;
        if args.seed.is_none() && !common.quiet {
            eprintln!("Live playback can include rejected attempts; their seeds will be reported.");
        }
        (Some(tx), Some(player))
    } else {
        (None, None)
    };
    let candidate = produce(&config, &args, |chunk| {
        if let Some(tx) = &tx {
            let _ = tx.send(chunk.to_vec());
        }
    })?;
    drop(tx);
    if let Some(player) = player {
        player.wait();
    }
    // Candidate audio is not trimmed, denoised or regenerated after selection.
    if let Some(path) = &out {
        write_candidate(path, &candidate.samples, candidate.stats.sample_rate)?;
    }
    report(
        common,
        &args,
        out.as_deref(),
        &candidate,
        started.elapsed().as_secs_f64(),
    );
    Ok(())
}

/// Resolve language/output rate and reject an unusable request before any engine work.
fn validate(args: &DesignArgs, common: &CommonOpts) -> Result<crate::config::Config> {
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
    Ok(config)
}

/// Each attempt is one independent, reproducible sample. A sample that misses
/// EOS (e.g. a silence runaway) or is silent is rejected, never trimmed.
fn produce(
    config: &crate::config::Config,
    args: &DesignArgs,
    mut on_audio: impl FnMut(&[f32]) + Send,
) -> Result<Candidate> {
    let mut rejected = Vec::new();
    for seed in attempt_seeds(args.seed) {
        let mut samples = Vec::new();
        let (stats, prepare_s, model) =
            engine::design_streaming(config, &args.instruct, Some(seed), &args.text, |chunk| {
                forward_chunk(&mut samples, chunk, &mut on_audio);
            })?;
        match validate_candidate(&samples, stats.eos_reached) {
            Ok(()) => {
                return Ok(Candidate {
                    seed,
                    rejected,
                    samples,
                    stats,
                    prepare_s,
                    model,
                    language: spqx_core::worker::normalize_language(&config.voice.language),
                });
            }
            Err(error) => {
                eprintln!("VoiceDesign seed {seed} rejected: {error}");
                rejected.push(seed);
            }
        }
    }
    bail!(
        "no complete VoiceDesign candidate from seed(s) {rejected:?}; incomplete samples are not saved. Retry, or pick another --seed"
    )
}

fn forward_chunk(samples: &mut Vec<f32>, chunk: &[f32], on_audio: &mut impl FnMut(&[f32])) {
    samples.extend_from_slice(chunk);
    on_audio(chunk);
}

fn write_candidate(out: &std::path::Path, samples: &[f32], rate: u32) -> Result<()> {
    if let Some(parent) = out.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(out)
        .with_context(|| format!("creating candidate without overwriting {}", out.display()))?;
    file.write_all(&spqx_core::audio::write_wav_bytes(samples, rate)?)?;
    file.sync_all()?;
    Ok(())
}

fn report(
    common: &CommonOpts,
    args: &DesignArgs,
    out: Option<&std::path::Path>,
    c: &Candidate,
    elapsed_s: f64,
) {
    if common.json {
        println!(
            "{}",
            serde_json::json!({
                "operation": "voice_design", "backend": c.stats.backend,
                "model": c.model,
                "text": args.text, "description": args.instruct,
                "language": c.language,
                "seed": c.seed,
                "seed_source": if args.seed.is_some() { "explicit" } else { "random" },
                "rejected_seeds": c.rejected, "sample_rate": c.stats.sample_rate,
                "prepare_ms": c.prepare_s * 1000.0,
                "first_pcm_ms_after_prepare": c.stats.ttfa_s * 1000.0,
                "generation_wall_s": c.stats.wall_s,
                "total_wall_s": elapsed_s, "audio_s": c.stats.audio_s,
                "eos_reached": c.stats.eos_reached, "wav": out,
                "canonical_voice_id": null, "registry_promoted": false,
            })
        );
    } else if !common.quiet {
        eprintln!(
            "VoiceDesign candidate{} ({:.2}s audio, seed {}; no registry enrollment)",
            out.map(|path| format!(" -> {}", path.display()))
                .unwrap_or_default(),
            c.stats.audio_s,
            c.seed
        );
    }
}

/// Unseeded requests get up to three recorded random seeds; an explicit seed is
/// deterministic, so it gets exactly one attempt.
fn attempt_seeds(seed: Option<u64>) -> Vec<u64> {
    use std::hash::BuildHasher;
    match seed {
        Some(seed) => vec![seed],
        None => {
            let random = std::collections::hash_map::RandomState::new();
            (0..3u64)
                .map(|attempt| random.hash_one(attempt) & i64::MAX as u64)
                .collect()
        }
    }
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
    fn streamed_chunks_are_forwarded_without_changing_saved_audio() {
        let mut stored = Vec::new();
        let mut played = Vec::new();
        for chunk in [&[0.1, -0.2][..], &[0.3][..]] {
            forward_chunk(&mut stored, chunk, &mut |pcm| played.extend_from_slice(pcm));
        }
        assert_eq!(stored, vec![0.1, -0.2, 0.3]);
        assert_eq!(played, stored);
    }
    #[test]
    fn explicit_seed_is_single_attempt_and_random_seeds_are_valid() {
        assert_eq!(attempt_seeds(Some(4242)), [4242]);
        let seeds = attempt_seeds(None);
        assert_eq!(seeds.len(), 3);
        assert!(seeds.iter().all(|&s| i64::try_from(s).is_ok()));
        assert!(seeds[0] != seeds[1] && seeds[1] != seeds[2]);
    }
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
