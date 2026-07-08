//! `spqx serve` — run the binary-framed worker protocol behind the unified
//! CLI. The same worker loop as the standalone `spqx-tts-worker` binary
//! (`spqx_core::worker`), but with defaults drawn from the CLI config while
//! flags still override.

use anyhow::{Context, Result};
use clap::Args;
use spqx_core::worker::{self, WorkerConfig};

use crate::config::Config;
use crate::engine;
use crate::CommonOpts;

#[derive(Debug, Args)]
pub struct ServeArgs {
    /// Voice: a registry name (see `spqx voices`) or the config default.
    #[arg(long)]
    pub voice: Option<String>,
    /// Reference WAV (bypasses the registry).
    #[arg(long, value_name = "WAV")]
    pub ref_audio: Option<std::path::PathBuf>,
    /// Transcript of --ref-audio.
    #[arg(long, value_name = "TEXT")]
    pub ref_text: Option<String>,
    /// File containing the transcript of --ref-audio.
    #[arg(long, value_name = "PATH")]
    pub ref_text_file: Option<std::path::PathBuf>,
    /// Override the output PCM sample rate.
    #[arg(long)]
    pub output_sample_rate: Option<u32>,
    /// Output chunk size in samples.
    #[arg(long, default_value_t = 2048)]
    pub blocksize: usize,
}

pub fn run(args: ServeArgs, common: &CommonOpts) -> Result<()> {
    let config = Config::load(common.config.as_deref())?;
    let reference = engine::resolve_reference(
        &config,
        args.voice.as_deref(),
        args.ref_audio.as_deref(),
        args.ref_text.as_deref(),
        args.ref_text_file.as_deref(),
    )?;
    let model_path = engine::resolve_model_dir(&config)?;

    let worker_config = WorkerConfig {
        model_path,
        ref_audio: reference.wav,
        ref_text: reference.text,
        language: worker::normalize_language(&config.voice.language),
        temperature: config.sampling.temperature,
        top_k: config.sampling.top_k,
        max_new_tokens: 1536,
        output_sample_rate: args
            .output_sample_rate
            .unwrap_or(config.audio.sample_rate_hz),
        blocksize: args.blocksize.max(1),
        streaming_chunk_size: 4,
    };

    worker::run_stdio(worker_config).context("running the spqx worker")
}
