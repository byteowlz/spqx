//! Opt-in real-model test. Provision local assets and obtain compute admission externally.
#![cfg(feature = "qwentts")]
use anyhow::{ensure, Context, Result};
use spqx_core::{
    audio::write_wav_bytes,
    synthesis::{Backend, EngineOptions, Sampling, Session, Stop},
};
use std::{
    path::{Path, PathBuf},
    time::Instant,
};

const DESCRIPTION: &str = "A fictional adult male voice with clear native standard German pronunciation. Calm, friendly and conversational, moderately low pitch. No singing or whispering.";

#[test]
#[ignore = "requires explicit VoiceDesign GGUF assets, pinned runtime and fresh compute admission"]
fn native_voice_design_contract() -> Result<()> {
    let _guard = spqx_core::stdio::StdoutGuard::new()?;
    let root = PathBuf::from(std::env::var("SPQX_DESIGN_ASSETS")?);
    let output = PathBuf::from(std::env::var("SPQX_DESIGN_OUTPUT")?);
    std::fs::create_dir(&output).context("use a fresh private output directory")?;
    let options = EngineOptions {
        backend: Backend::Ggml,
        device: std::env::var("SPQX_DESIGN_DEVICE")?,
        library: Some(std::env::var("SPQX_DESIGN_LIBRARY")?.into()),
        codec: Some(root.join("qwen-tokenizer-12hz-F32.gguf")),
    };
    // Reject and release the wrong-model context BEFORE loading the design model.
    let wrong = Session::load_voice_design(
        &root.join("base.gguf"),
        DESCRIPTION,
        "german",
        Some(4242),
        &options,
    );
    ensure!(wrong
        .err()
        .context("Base accepted as VoiceDesign")?
        .to_string()
        .contains("requires voice_design"));
    let prepare = Instant::now();
    let session = Session::load_voice_design(
        &root.join("qwen-talker-1.7b-voicedesign-Q8_0.gguf"),
        DESCRIPTION,
        "german",
        Some(4242),
        &options,
    )?;
    eprintln!(
        "VoiceDesign prepare_s={:.6}",
        prepare.elapsed().as_secs_f64()
    );
    ensure!(session.backend_name() == "ggml");
    let sampling = Sampling {
        temperature: 0.9,
        top_k: 50,
        max_new_tokens: 256,
        chunk_size: 4,
    };
    let texts = [
        "Die Spracherkennung läuft direkt auf deinem Gerät. Heute probieren wir eine neue Stimme aus.",
        "Hallo, schön, dass du da bist. Morgen fahren wir nach München.",
    ];
    render(
        &session,
        &sampling,
        texts[0],
        &output.join("candidate-0.wav"),
    )?;
    check_interruptions(&session, &sampling, texts[0])?;
    // EOS after active cancellation, cap and malformed input proves retained-session recovery.
    render(
        &session,
        &sampling,
        texts[1],
        &output.join("candidate-1.wav"),
    )?;
    Ok(())
}

fn check_interruptions(session: &Session, sampling: &Sampling, text: &str) -> Result<()> {
    ensure!(session.synthesize(text, sampling, |_, _| true, || true)? == Stop::Cancelled);
    ensure!(session
        .synthesize("invalid\0text", sampling, |_, _| true, || false)
        .is_err());
    let mut calls = 0;
    let stop = session.synthesize(
        text,
        sampling,
        |_, _| {
            calls += 1;
            false
        },
        || false,
    )?;
    ensure!(
        stop == Stop::Cancelled && calls == 1,
        "callback cancellation not fenced"
    );
    let cap = Sampling {
        max_new_tokens: 2,
        ..sampling.clone()
    };
    ensure!(session.synthesize(text, &cap, |_, _| true, || false)? == Stop::CapReached);
    Ok(())
}

fn render(session: &Session, sampling: &Sampling, text: &str, path: &Path) -> Result<()> {
    let start = Instant::now();
    let mut first_pcm = None;
    let mut samples = Vec::new();
    let stop = session.synthesize(
        text,
        sampling,
        |chunk, rate| {
            assert_eq!(rate, 24000);
            first_pcm.get_or_insert_with(|| start.elapsed().as_secs_f64());
            samples.extend_from_slice(chunk);
            true
        },
        || false,
    )?;
    ensure!(stop == Stop::Eos, "incomplete candidate: {stop:?}");
    ensure!(samples.len() >= 24000 && samples.len() <= 20 * 24000);
    ensure!(samples.iter().all(|v| v.is_finite() && v.abs() <= 1.0));
    ensure!(samples.iter().any(|v| v.abs() > 1e-4), "silent candidate");
    std::fs::write(path, write_wav_bytes(&samples, 24000)?)?;
    std::fs::write(path.with_extension("txt"), text)?;
    eprintln!(
        "candidate={} first_pcm_s={:.6} complete_s={:.6} audio_s={:.3} EOS",
        path.display(),
        first_pcm.unwrap(),
        start.elapsed().as_secs_f64(),
        samples.len() as f64 / 24000.0
    );
    Ok(())
}
