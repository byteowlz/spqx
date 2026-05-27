// Copyright 2026 Michael Yuan.
// SPDX-License-Identifier: Apache-2.0

//! Persistent JSON-lines worker for Qwen3 TTS voice cloning.
//!
//! Stdin requests:
//!   {"id":"1","text":"Hallo"}
//!
//! Stdout events:
//!   {"type":"server_ready",...}
//!   {"type":"audio_chunk","id":"1","data":"...base64 pcm_s16le..."}
//!   {"type":"generated","id":"1","seconds":1.23,"audioSeconds":2.34,"rtf":0.53}
//!   {"type":"request_done","id":"1","contentType":"audio/pcm"}

use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine};
use clap::Parser;
use qwen3_tts_rs::audio::{load_wav_file, resample};
use qwen3_tts_rs::audio_encoder::AudioEncoder;
use qwen3_tts_rs::inference::TTSInference;
use qwen3_tts_rs::speaker_encoder::SpeakerEncoder;
use qwen3_tts_rs::tensor::{Device, Tensor};
use serde::Deserialize;
use serde_json::json;
use std::fs::File;
use std::io::{self, BufRead, LineWriter, Write};
use std::os::fd::{FromRawFd, RawFd};
use std::path::{Path, PathBuf};
use std::time::Instant;

const STDOUT_FILENO: i32 = 1;
const STDERR_FILENO: i32 = 2;

extern "C" {
    fn dup(fd: i32) -> i32;
    fn dup2(oldfd: i32, newfd: i32) -> i32;
}

#[derive(Parser, Debug)]
#[command(name = "worker", about = "Persistent Qwen3 TTS JSON-lines worker")]
struct Args {
    /// Path to the model directory.
    model_path: PathBuf,

    /// Reference WAV for ICL voice cloning.
    #[arg(long)]
    ref_audio: PathBuf,

    /// Reference transcript.
    #[arg(long)]
    ref_text: Option<String>,

    /// File containing the reference transcript.
    #[arg(long)]
    ref_text_file: Option<PathBuf>,

    /// Target language, e.g. german, english.
    #[arg(long, default_value = "german")]
    language: String,

    /// Sampling temperature.
    #[arg(long, default_value_t = 0.9)]
    temperature: f64,

    /// Top-k sampling.
    #[arg(long, default_value_t = 50)]
    top_k: i64,

    /// Maximum generated codec frames.
    #[arg(long, default_value_t = 2048)]
    max_codes: i64,

    /// PCM output sample rate.
    #[arg(long, default_value_t = 24000)]
    output_sample_rate: u32,

    /// Output chunk size in samples.
    #[arg(long, default_value_t = 512)]
    blocksize: usize,
}

#[derive(Debug, Deserialize)]
struct Request {
    id: serde_json::Value,
    text: String,
    language: Option<String>,
    temperature: Option<f64>,
    top_k: Option<i64>,
    max_codes: Option<i64>,
}

struct JsonWriter {
    inner: LineWriter<File>,
}

impl JsonWriter {
    fn new(fd: RawFd) -> Self {
        let file = unsafe { File::from_raw_fd(fd) };
        Self {
            inner: LineWriter::new(file),
        }
    }

    fn write(&mut self, value: serde_json::Value) -> anyhow::Result<()> {
        serde_json::to_writer(&mut self.inner, &value)?;
        self.inner.write_all(b"\n")?;
        self.inner.flush()?;
        Ok(())
    }
}

struct Worker {
    inference: TTSInference,
    speaker_embedding: Tensor,
    ref_codes: Vec<Vec<i64>>,
    ref_text: String,
    language: String,
    temperature: f64,
    top_k: i64,
    max_codes: i64,
    output_sample_rate: u32,
    blocksize: usize,
}

impl Worker {
    fn load(args: &Args) -> anyhow::Result<Self> {
        #[cfg(feature = "mlx")]
        {
            qwen3_tts_rs::backend::mlx::stream::init_mlx(true);
            eprintln!("MLX backend initialized (Metal GPU)");
        }

        let device = Device::Cpu;
        let inference = TTSInference::new(&args.model_path, device)?;
        let speaker_encoder = SpeakerEncoder::load(
            inference.weights(),
            &inference.config().speaker_encoder_config,
            device,
        )?;
        let tokenizer_path = args
            .model_path
            .join("speech_tokenizer")
            .join("model.safetensors");
        let audio_encoder = AudioEncoder::load(&tokenizer_path, device)?;
        let ref_text = load_ref_text(args)?;
        let se_sr = inference.config().speaker_encoder_config.sample_rate;
        let (samples, sample_rate) = load_wav_file(path_str(&args.ref_audio)?)?;
        let samples = if sample_rate == se_sr {
            samples
        } else {
            resample(&samples, sample_rate, se_sr)?
        };
        let speaker_embedding = speaker_encoder.extract_embedding(&samples)?;
        let ref_codes = audio_encoder.encode(&samples)?;

        Ok(Self {
            inference,
            speaker_embedding,
            ref_codes,
            ref_text,
            language: args.language.clone(),
            temperature: args.temperature,
            top_k: args.top_k,
            max_codes: args.max_codes,
            output_sample_rate: args.output_sample_rate,
            blocksize: args.blocksize,
        })
    }

    fn handle(&self, request: &Request, writer: &mut JsonWriter) -> anyhow::Result<()> {
        let text = request.text.trim();
        if text.is_empty() {
            anyhow::bail!("empty text");
        }

        let started = Instant::now();
        let language = request.language.as_deref().unwrap_or(&self.language);
        let temperature = request.temperature.unwrap_or(self.temperature);
        let top_k = request.top_k.unwrap_or(self.top_k);
        let max_codes = request.max_codes.unwrap_or(self.max_codes);
        let (samples, sample_rate) = self.inference.generate_with_icl(
            text,
            &self.ref_text,
            &self.ref_codes,
            &self.speaker_embedding,
            language,
            temperature,
            top_k,
            max_codes,
        )?;
        let samples = if sample_rate == self.output_sample_rate {
            samples
        } else {
            resample(&samples, sample_rate, self.output_sample_rate)?
        };
        let elapsed = started.elapsed().as_secs_f64();
        let audio_seconds = samples.len() as f64 / self.output_sample_rate as f64;

        writer.write(json!({
            "type": "ttfa",
            "id": request.id,
            "seconds": round3(elapsed),
            "label": "rust_worker_full"
        }))?;
        for chunk in samples.chunks(self.blocksize) {
            let bytes = samples_to_pcm_bytes(chunk);
            writer.write(json!({
                "type": "audio_chunk",
                "id": request.id,
                "data": BASE64_STANDARD.encode(bytes)
            }))?;
        }
        writer.write(json!({
            "type": "generated",
            "id": request.id,
            "seconds": round3(elapsed),
            "audioSeconds": round3(audio_seconds),
            "rtf": round3(elapsed / audio_seconds),
            "label": "rust_worker_full"
        }))?;
        clear_mlx_cache();
        writer.write(json!({
            "type": "request_done",
            "id": request.id,
            "contentType": "audio/pcm"
        }))?;
        Ok(())
    }
}

fn main() -> anyhow::Result<()> {
    let stdout_fd = unsafe { dup(STDOUT_FILENO) };
    if stdout_fd < 0 {
        anyhow::bail!("failed to duplicate stdout");
    }
    if unsafe { dup2(STDERR_FILENO, STDOUT_FILENO) } < 0 {
        anyhow::bail!("failed to redirect stdout to stderr");
    }
    let mut writer = JsonWriter::new(stdout_fd);
    let args = Args::parse();

    match Worker::load(&args) {
        Ok(worker) => serve(worker, &mut writer),
        Err(error) => {
            writer.write(json!({ "type": "error", "message": error.to_string() }))?;
            Err(error)
        }
    }
}

fn serve(worker: Worker, writer: &mut JsonWriter) -> anyhow::Result<()> {
    writer.write(json!({
        "type": "server_ready",
        "backend": "rust_mlx",
        "sampleRate": worker.output_sample_rate,
        "blocksize": worker.blocksize,
        "language": worker.language,
        "temperature": worker.temperature,
        "topK": worker.top_k,
        "maxCodes": worker.max_codes
    }))?;

    let stdin = io::stdin();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<Request>(&line) {
            Ok(request) => {
                if let Err(error) = worker.handle(&request, writer) {
                    writer.write(json!({
                        "type": "request_error",
                        "id": request.id,
                        "message": error.to_string()
                    }))?;
                }
            }
            Err(error) => {
                writer.write(json!({
                    "type": "request_error",
                    "id": null,
                    "message": error.to_string()
                }))?;
            }
        }
    }
    Ok(())
}

fn load_ref_text(args: &Args) -> anyhow::Result<String> {
    if let Some(path) = &args.ref_text_file {
        return Ok(std::fs::read_to_string(path)?.trim().to_string());
    }
    if let Some(text) = &args.ref_text {
        return Ok(text.trim().to_string());
    }
    anyhow::bail!("provide --ref-text or --ref-text-file")
}

fn path_str(path: &Path) -> anyhow::Result<&str> {
    path.to_str()
        .ok_or_else(|| anyhow::anyhow!("path is not valid UTF-8: {}", path.display()))
}

fn samples_to_pcm_bytes(samples: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(samples.len() * 2);
    for &sample in samples {
        let scaled = (sample.clamp(-1.0, 1.0) * 32767.0) as i16;
        bytes.extend_from_slice(&scaled.to_le_bytes());
    }
    bytes
}

fn clear_mlx_cache() {
    #[cfg(feature = "mlx")]
    unsafe {
        qwen3_tts_rs::backend::mlx::ffi::mlx_clear_cache();
    }
}

fn round3(value: f64) -> f64 {
    (value * 1000.0).round() / 1000.0
}
