//! `spqx-tts-worker` — persistent binary-framed Qwen3-TTS worker.
//!
//! A thin clap shell over `spqx_core::worker`; the worker loop, frame
//! protocol, and streaming live in the library so `spqx serve` shares one
//! implementation. Flag-only and config-file-free by design.
//!
//! `pibot-tts-worker` builds from this same source as an upstream-name alias.

use std::path::PathBuf;

use clap::Parser;
use spqx_core::worker::{self, WorkerConfig};

const DEFAULT_MODEL_PATH: &str = "/tmp/qwen3-tts.cpp/models/Qwen3-TTS-12Hz-0.6B-Base";
const DEFAULT_REF_TEXT: &str = "I'm confused why some people have super short timelines, yet at the same time are bullish on scaling up reinforcement learning atop LLMs. If we're actually close to a human-like learner, then this whole approach of training on verifiable outcomes.";
const DEFAULT_MAX_NEW_TOKENS: i64 = 1536;
const DEFAULT_BLOCKSIZE: usize = 512;

#[derive(Parser, Debug)]
#[command(
    name = "spqx-tts-worker",
    about = "Persistent Qwen3 TTS binary-framed worker"
)]
struct Args {
    /// Run as a persistent binary-framed worker on stdin/stdout.
    #[arg(long)]
    serve: bool,

    /// Path to the model directory. `--model-name` is accepted as the Python-compatible alias.
    #[arg(long, alias = "model-name", default_value = DEFAULT_MODEL_PATH)]
    model_path: PathBuf,

    /// Target text for one-shot generation.
    #[arg(long)]
    text: Option<String>,

    /// File containing target text for one-shot generation.
    #[arg(long)]
    text_file: Option<PathBuf>,

    /// One-shot output WAV path.
    #[arg(long, default_value = "data/voices/qwen3-rust-worker-test.wav")]
    output: PathBuf,

    /// Accepted for Python worker CLI compatibility.
    #[arg(long, default_value = "")]
    output_dir: String,

    /// Reference WAV for ICL voice cloning.
    #[arg(long)]
    ref_audio: PathBuf,

    /// Reference transcript.
    #[arg(long)]
    ref_text: Option<String>,

    /// File containing the reference transcript.
    #[arg(long)]
    ref_text_file: Option<PathBuf>,

    /// Accepted for Python worker CLI compatibility.
    #[arg(long, default_value = "Aiden")]
    speaker: String,

    /// Accepted for Python worker CLI compatibility.
    #[arg(long)]
    instruct: Option<String>,

    /// Target language, e.g. german, english, de.
    #[arg(long, default_value = "german")]
    language: String,

    /// Accepted for Python worker CLI compatibility. Rust worker currently uses local full weights.
    #[arg(long, default_value = "6bit")]
    mlx_quantization: String,

    /// Accepted for Python worker CLI compatibility.
    #[arg(long)]
    streaming_chunk_size: Option<i64>,

    /// Maximum generated codec frames.
    #[arg(long, default_value_t = DEFAULT_MAX_NEW_TOKENS)]
    max_new_tokens: i64,

    /// Output chunk size in samples.
    #[arg(long, default_value_t = DEFAULT_BLOCKSIZE)]
    blocksize: usize,

    /// PCM output sample rate.
    #[arg(long, default_value_t = 16000)]
    output_sample_rate: u32,

    /// Accepted for Python worker CLI compatibility. Rust worker does not currently seed MLX RNG.
    #[arg(long)]
    seed: Option<u64>,

    /// Sampling temperature.
    #[arg(long, default_value_t = 0.9)]
    temperature: f64,

    /// Top-k sampling.
    #[arg(long, default_value_t = 50)]
    top_k: i64,

    /// Accepted for Python worker CLI compatibility.
    #[arg(long, default_value_t = 1.0)]
    top_p: f64,

    /// Accepted for Python worker CLI compatibility.
    #[arg(long, default_value_t = 1.05)]
    repetition_penalty: f64,

    /// Accepted for Python worker CLI compatibility.
    #[arg(long, default_value_t = 1.0)]
    speed: f64,

    /// Accepted for Python worker CLI compatibility.
    #[arg(long, default_value = "cuda")]
    device: String,

    /// Accepted for Python worker CLI compatibility.
    #[arg(long, default_value = "auto")]
    dtype: String,

    /// Accepted for Python worker CLI compatibility.
    #[arg(long, default_value = "eager")]
    attn_implementation: String,

    /// Accepted for Python worker CLI compatibility.
    #[arg(long)]
    xvec_only: bool,

    /// Accepted for Python worker CLI compatibility.
    #[arg(long)]
    parity_mode: bool,

    /// Accepted for Python worker CLI compatibility.
    #[arg(long, default_value_t = true)]
    non_streaming_mode: bool,
}

fn expanduser(path: &std::path::Path) -> PathBuf {
    let text = path.to_string_lossy();
    if let Some(rest) = text.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    path.to_path_buf()
}

fn resolve_ref_text(args: &Args) -> anyhow::Result<String> {
    if let Some(path) = &args.ref_text_file {
        return Ok(std::fs::read_to_string(expanduser(path))?.trim().to_string());
    }
    if let Some(text) = &args.ref_text {
        return Ok(text.trim().to_string());
    }
    Ok(DEFAULT_REF_TEXT.to_string())
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let config = WorkerConfig {
        model_path: expanduser(&args.model_path),
        ref_audio: expanduser(&args.ref_audio),
        ref_text: resolve_ref_text(&args)?,
        language: worker::normalize_language(&args.language),
        temperature: args.temperature,
        top_k: args.top_k,
        max_new_tokens: args.max_new_tokens,
        output_sample_rate: args.output_sample_rate,
        blocksize: args.blocksize.max(1),
        streaming_chunk_size: args.streaming_chunk_size.unwrap_or(4).max(1) as usize,
    };

    if args.serve {
        worker::run_stdio(config)
    } else {
        let text = match (&args.text, &args.text_file) {
            (Some(text), _) => text.trim().to_string(),
            (None, Some(file)) => std::fs::read_to_string(expanduser(file))?.trim().to_string(),
            (None, None) => anyhow::bail!("provide --text or --text-file"),
        };
        worker::generate_to_wav(&config, &text, &expanduser(&args.output))
    }
}
