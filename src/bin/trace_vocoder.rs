// Copyright 2026 Michael Yuan.
// SPDX-License-Identifier: Apache-2.0

//! Trace the Rust/MLX Qwen3-TTS vocoder for GGUF parity checks.

use clap::Parser;
use qwen3_tts_rs::tensor::{Device, Tensor};
use qwen3_tts_rs::trace::TraceWriter;
use qwen3_tts_rs::vocoder::{load_vocoder_weights, Vocoder, VocoderConfig};
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Parser, Debug)]
#[command(name = "trace_vocoder", about = "Trace Rust Qwen3-TTS vocoder output")]
struct Args {
    /// Path to the local Qwen3-TTS model directory.
    #[arg(long, default_value = "~/models/qwen3-tts-12hz-0.6b-base")]
    model_path: PathBuf,

    /// Codes JSON with shape [1, 16, frames].
    #[arg(long)]
    codes_json: PathBuf,

    /// Optional trace output directory.
    #[arg(long)]
    trace_dir: Option<PathBuf>,

    /// Optional raw waveform sample JSON output.
    #[arg(long)]
    waveform_out: Option<PathBuf>,

    /// Number of first/last tensor values to record.
    #[arg(long, default_value_t = 8)]
    sample_count: usize,
}

fn main() -> anyhow::Result<()> {
    #[cfg(feature = "mlx")]
    {
        qwen3_tts_rs::backend::mlx::stream::init_mlx(true);
    }

    let args = Args::parse();
    if args.trace_dir.is_none() && args.waveform_out.is_none() {
        anyhow::bail!("at least one of --trace-dir or --waveform-out is required");
    }

    let model_path = args.model_path.expanduser();
    let weights = load_vocoder_weights(
        model_path
            .join("speech_tokenizer")
            .join("model.safetensors"),
        Device::Cpu,
    )?;
    let vocoder = Vocoder::load(&weights, VocoderConfig::default(), Device::Cpu)?;
    let codes = load_codes(&args.codes_json.expanduser())?;
    let waveform = vocoder.decode(&codes);

    if let Some(trace_dir) = args.trace_dir.as_ref() {
        let mut trace = TraceWriter::create(trace_dir.expanduser(), args.sample_count)?;
        trace.tensor("vocoder/waveform", &waveform)?;
    }

    if let Some(path) = args.waveform_out.as_ref() {
        write_waveform_json(&path.expanduser(), &waveform.contiguous().to_vec_f32())?;
    }

    Ok(())
}

fn load_codes(path: &Path) -> anyhow::Result<Tensor> {
    let values: Vec<Vec<Vec<i64>>> = serde_json::from_str(&std::fs::read_to_string(path)?)?;
    let batch = values.len();
    if batch != 1 {
        anyhow::bail!("codes JSON must have batch size 1, got {batch}");
    }
    let quantizers = values[0].len();
    if quantizers != 16 {
        anyhow::bail!("codes JSON must have 16 quantizers, got {quantizers}");
    }
    let frames = values[0]
        .first()
        .map(Vec::len)
        .ok_or_else(|| anyhow::anyhow!("codes JSON contains no quantizers"))?;
    let mut flat = Vec::with_capacity(batch * quantizers * frames);
    for quantizer in &values[0] {
        if quantizer.len() != frames {
            anyhow::bail!("codes JSON quantizers have inconsistent frame counts");
        }
        flat.extend(quantizer.iter().copied());
    }
    Ok(Tensor::from_slice_i64(&flat).view(&[1, quantizers as i64, frames as i64]))
}

fn write_waveform_json(path: &Path, values: &[f32]) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut out = std::fs::File::create(path)?;
    serde_json::to_writer(&mut out, values)?;
    out.write_all(b"\n")?;
    Ok(())
}

trait ExpandUser {
    fn expanduser(&self) -> PathBuf;
}

impl ExpandUser for PathBuf {
    fn expanduser(&self) -> PathBuf {
        let text = self.to_string_lossy();
        if let Some(rest) = text.strip_prefix("~/") {
            if let Some(home) = std::env::var_os("HOME") {
                return PathBuf::from(home).join(rest);
            }
        }
        self.clone()
    }
}
