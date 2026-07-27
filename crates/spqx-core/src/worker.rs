//! Persistent binary-framed TTS worker, shared by the standalone
//! `spqx-tts-worker` binary and `spqx serve`.
//!
//! Frame header: `u8 type, u32 request_id, u32 payload_len` (little-endian),
//! then payload. Input frames: speak(1)/cancel(2)/shutdown(3). Output frames:
//! ready(1)/audio_start(2)/audio_chunk(3)/audio_done(4)/error(5). PCM audio
//! is little-endian i16 mono. Status events are JSON on stderr.
//!
//! The worker is deliberately config-file-free: everything comes from an
//! explicit [`WorkerConfig`], so low-latency hosts (foxline) construct it from
//! flags and the CLI constructs it from its config + flags — one code path.

use std::collections::HashSet;
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::{FromRawFd, RawFd};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Instant;

use anyhow::Result;
use serde_json::json;

use crate::audio::{load_wav_file, resample, write_wav_file};
use crate::audio_encoder::AudioEncoder;
use crate::inference::{IclSession, TTSInference};
use crate::speaker_encoder::SpeakerEncoder;
use crate::tensor::{Device, Tensor};

const FRAME_HEADER_BYTES: usize = 9;
const STDOUT_FILENO: i32 = 1;
const STDERR_FILENO: i32 = 2;

pub const WORKER_INPUT_SPEAK: u8 = 1;
pub const WORKER_INPUT_CANCEL: u8 = 2;
pub const WORKER_INPUT_SHUTDOWN: u8 = 3;
pub const WORKER_OUTPUT_READY: u8 = 1;
pub const WORKER_OUTPUT_AUDIO_START: u8 = 2;
pub const WORKER_OUTPUT_AUDIO_CHUNK: u8 = 3;
pub const WORKER_OUTPUT_AUDIO_DONE: u8 = 4;
pub const WORKER_OUTPUT_ERROR: u8 = 5;

extern "C" {
    fn dup(fd: i32) -> i32;
    fn dup2(oldfd: i32, newfd: i32) -> i32;
}

/// Everything needed to build a worker. Resolved by the caller (binary or CLI)
/// from flags/config — no config-file reading happens here.
#[derive(Debug, Clone)]
pub struct WorkerConfig {
    /// Local model directory.
    pub model_path: PathBuf,
    /// Reference audio path (ICL voice cloning).
    pub ref_audio: PathBuf,
    /// Reference transcript (already resolved from text/file).
    pub ref_text: String,
    /// Target language (already normalized; `auto` allowed).
    pub language: String,
    pub temperature: f64,
    pub top_k: i64,
    pub max_new_tokens: i64,
    pub output_sample_rate: u32,
    pub blocksize: usize,
    pub streaming_chunk_size: usize,
}

struct Frame {
    frame_type: u8,
    request_id: u32,
    payload: Vec<u8>,
}

struct BinaryWriter {
    inner: File,
}

impl BinaryWriter {
    fn new(fd: RawFd) -> Self {
        Self {
            inner: unsafe { File::from_raw_fd(fd) },
        }
    }

    fn write_frame(&mut self, frame_type: u8, request_id: u32, payload: &[u8]) -> Result<()> {
        let payload_len = u32::try_from(payload.len())?;
        let mut header = [0u8; FRAME_HEADER_BYTES];
        header[0] = frame_type;
        header[1..5].copy_from_slice(&request_id.to_le_bytes());
        header[5..9].copy_from_slice(&payload_len.to_le_bytes());
        self.inner.write_all(&header)?;
        self.inner.write_all(payload)?;
        self.inner.flush()?;
        Ok(())
    }
}

struct Worker {
    model_name: String,
    inference: TTSInference,
    icl_session: IclSession,
    #[allow(dead_code)]
    speaker_embedding: Tensor,
    #[allow(dead_code)]
    ref_codes: Vec<Vec<i64>>,
    #[allow(dead_code)]
    ref_text: String,
    #[allow(dead_code)]
    language: String,
    temperature: f64,
    top_k: i64,
    max_new_tokens: i64,
    output_sample_rate: u32,
    blocksize: usize,
    streaming_chunk_size: usize,
}

impl Worker {
    fn load(config: &WorkerConfig) -> Result<Self> {
        #[cfg(feature = "mlx")]
        {
            crate::backend::mlx::stream::init_mlx(true);
            eprintln!("MLX backend initialized (Metal GPU)");
        }

        if !config.model_path.exists() {
            anyhow::bail!(
                "worker requires a local model directory, got {}",
                config.model_path.display()
            );
        }

        let device = Device::Cpu;
        let inference = TTSInference::new(&config.model_path, device)?;
        let speaker_encoder = SpeakerEncoder::load(
            inference.weights(),
            &inference.config().speaker_encoder_config,
            device,
        )?;
        let tokenizer_path = config
            .model_path
            .join("speech_tokenizer")
            .join("model.safetensors");
        let audio_encoder = AudioEncoder::load(&tokenizer_path, device)?;
        let se_sr = inference.config().speaker_encoder_config.sample_rate;
        let (samples, sample_rate) = load_wav_file(path_str(&config.ref_audio)?)?;
        let samples = if sample_rate == se_sr {
            samples
        } else {
            resample(&samples, sample_rate, se_sr)?
        };
        let speaker_embedding = speaker_encoder.extract_embedding(&samples)?;
        let ref_codes = audio_encoder.encode(&samples)?;
        // Precompute the reference side of the ICL prompt once; requests only
        // pay for their own synthesis tokens.
        let icl_session = inference.prepare_icl_session(
            &config.ref_text,
            &ref_codes,
            &speaker_embedding,
            &config.language,
        )?;

        Ok(Self {
            model_name: config.model_path.display().to_string(),
            inference,
            icl_session,
            speaker_embedding,
            ref_codes,
            ref_text: config.ref_text.clone(),
            language: config.language.clone(),
            temperature: config.temperature,
            top_k: config.top_k,
            max_new_tokens: config.max_new_tokens,
            output_sample_rate: config.output_sample_rate,
            blocksize: config.blocksize.max(1),
            streaming_chunk_size: config.streaming_chunk_size.max(1),
        })
    }

    fn handle_speak(
        &self,
        request_id: u32,
        payload: &[u8],
        writer: &mut BinaryWriter,
        cancelled: &Arc<Mutex<HashSet<u32>>>,
    ) -> Result<()> {
        if take_cancelled(cancelled, request_id) {
            writer.write_frame(WORKER_OUTPUT_AUDIO_DONE, request_id, &[])?;
            return Ok(());
        }

        let text = std::str::from_utf8(payload)?.trim();
        if text.is_empty() {
            anyhow::bail!("empty text");
        }

        writer.write_frame(
            WORKER_OUTPUT_AUDIO_START,
            request_id,
            &self.output_sample_rate.to_le_bytes(),
        )?;

        log_json(json!({
            "type": "ready",
            "backend": "rust_mlx",
            "model": self.model_name,
            "modelType": "base",
            "chunkSize": self.streaming_chunk_size,
            "maxNewTokens": self.max_new_tokens,
            "temperature": self.temperature,
            "topK": self.top_k,
        }));

        let started = Instant::now();
        let mut streamer = PcmStreamer::new(self.output_sample_rate, self.blocksize);
        let mut ttfa_logged = false;
        let mut stream_error: Option<anyhow::Error> = None;
        let mut cancelled_during_stream = false;

        let stop = self.inference.generate_with_icl_session_streaming(
            &self.icl_session,
            text,
            self.temperature,
            self.top_k,
            self.max_new_tokens,
            self.streaming_chunk_size,
            |samples, sample_rate| {
                if take_cancelled(cancelled, request_id) {
                    cancelled_during_stream = true;
                    return false;
                }
                let gated;
                let samples: &[f32] = if ttfa_logged {
                    samples
                } else {
                    log_json(json!({
                        "type": "ttfa",
                        "seconds": round3(started.elapsed().as_secs_f64()),
                        "label": "voice_clone_rust"
                    }));
                    ttfa_logged = true;
                    let mut buffer = samples.to_vec();
                    // SPQX_NO_GATE=1 bypasses output cleanup for debugging.
                    if std::env::var_os("SPQX_NO_GATE").is_none() {
                        crate::postprocess::gate_leading_nonspeech(&mut buffer, sample_rate);
                    }
                    gated = buffer;
                    &gated
                };
                if let Err(error) = streamer.push(samples, sample_rate, request_id, writer) {
                    stream_error = Some(error);
                    return false;
                }
                true
            },
        )?;
        if let Some(error) = stream_error {
            return Err(error);
        }
        if cancelled_during_stream {
            log_json(json!({ "type": "request_cancelled", "id": request_id }));
            writer.write_frame(WORKER_OUTPUT_AUDIO_DONE, request_id, &[])?;
            clear_mlx_cache();
            return Ok(());
        }
        streamer.finish(request_id, writer)?;

        let elapsed = started.elapsed().as_secs_f64();
        let audio_seconds = streamer.audio_samples as f64 / self.output_sample_rate as f64;
        let trailing_s = streamer.trailing_nonspeech_seconds();
        log_json(json!({
            "type": "generated",
            "seconds": round3(elapsed),
            "audioSeconds": round3(audio_seconds),
            "rtf": round3(audio_seconds / elapsed.max(f64::EPSILON)),
            "stop": stop.as_str(),
            "trailingSilenceSeconds": round3(trailing_s),
            "label": "voice_clone_rust"
        }));
        clear_cancelled(cancelled, request_id);
        clear_mlx_cache();

        // The audio is already streamed, so this cannot un-send it — but the
        // caller must not treat a truncated or collapsed utterance as a
        // complete one. Both failures are silent otherwise: the model stops
        // speaking and the run still looks successful.
        if let Some(reason) = degenerate_reason(stop, trailing_s) {
            log_json(json!({ "type": "degenerate", "reason": reason }));
            writer.write_frame(WORKER_OUTPUT_ERROR, request_id, reason.as_bytes())?;
            return Ok(());
        }
        writer.write_frame(WORKER_OUTPUT_AUDIO_DONE, request_id, &[])?;
        Ok(())
    }
}

struct PcmStreamer {
    output_sample_rate: u32,
    blocksize: usize,
    found_speech: bool,
    leftover: Vec<i16>,
    audio_samples: usize,
    /// Rolling window of the most recent output, kept so the end of the
    /// utterance can be inspected after streaming. Bounded because a
    /// collapsed generation can run for minutes.
    tail: Vec<i16>,
}

impl PcmStreamer {
    /// Longer than any tail worth reporting, short enough to stay cheap.
    const TAIL_SECONDS: usize = 30;

    fn new(output_sample_rate: u32, blocksize: usize) -> Self {
        Self {
            output_sample_rate,
            blocksize,
            found_speech: false,
            leftover: Vec::new(),
            audio_samples: 0,
            tail: Vec::new(),
        }
    }

    fn remember_tail(&mut self, chunk: &[i16]) {
        self.tail.extend_from_slice(chunk);
        let cap = self.output_sample_rate as usize * Self::TAIL_SECONDS;
        if self.tail.len() > cap {
            self.tail.drain(0..self.tail.len() - cap);
        }
    }

    fn trailing_nonspeech_seconds(&self) -> f64 {
        let floats: Vec<f32> = self
            .tail
            .iter()
            .map(|s| *s as f32 / i16::MAX as f32)
            .collect();
        crate::postprocess::trailing_nonspeech_seconds(&floats, self.output_sample_rate)
    }

    fn push(
        &mut self,
        samples: &[f32],
        sample_rate: u32,
        request_id: u32,
        writer: &mut BinaryWriter,
    ) -> Result<()> {
        let samples = if sample_rate == self.output_sample_rate {
            samples.to_vec()
        } else {
            resample(samples, sample_rate, self.output_sample_rate)?
        };
        let mut pcm = samples_to_i16(&samples);
        if !self.found_speech && std::env::var_os("SPQX_NO_GATE").is_some() {
            self.found_speech = true;
        }
        if !self.found_speech {
            // Low threshold and generous preroll: a soft unvoiced plosive at
            // utterance start ("T" in "Tune") sits well below typical speech
            // level, and trimming into it drops the first consonant.
            let threshold = (32768.0 * 0.005) as i16;
            if let Some(first_speech) = pcm.iter().position(|sample| sample.abs() > threshold) {
                let preroll = (self.output_sample_rate as f64 * 0.080) as usize;
                let start = first_speech.saturating_sub(preroll);
                pcm.drain(0..start);
                self.found_speech = true;
            } else {
                return Ok(());
            }
        }

        if !self.leftover.is_empty() {
            let mut combined = Vec::with_capacity(self.leftover.len() + pcm.len());
            combined.append(&mut self.leftover);
            combined.append(&mut pcm);
            pcm = combined;
        }

        let complete = (pcm.len() / self.blocksize) * self.blocksize;
        for chunk in pcm[..complete].chunks(self.blocksize) {
            self.audio_samples += chunk.len();
            self.remember_tail(chunk);
            writer.write_frame(WORKER_OUTPUT_AUDIO_CHUNK, request_id, &i16_bytes(chunk))?;
        }
        self.leftover = pcm[complete..].to_vec();
        Ok(())
    }

    fn finish(&mut self, request_id: u32, writer: &mut BinaryWriter) -> Result<()> {
        if self.leftover.is_empty() {
            return Ok(());
        }
        self.audio_samples += self.leftover.len();
        let mut chunk = std::mem::take(&mut self.leftover);
        chunk.resize(self.blocksize, 0);
        self.remember_tail(&chunk);
        writer.write_frame(WORKER_OUTPUT_AUDIO_CHUNK, request_id, &i16_bytes(&chunk))?;
        Ok(())
    }
}

/// Run the persistent worker on stdin/stdout. Redirects the engine's stdout
/// chatter to stderr so the real stdout carries only binary frames. Blocks
/// until a shutdown frame or EOF.
pub fn run_stdio(config: WorkerConfig) -> Result<()> {
    let stdout_fd = unsafe { dup(STDOUT_FILENO) };
    if stdout_fd < 0 {
        anyhow::bail!("failed to duplicate stdout");
    }
    if unsafe { dup2(STDERR_FILENO, STDOUT_FILENO) } < 0 {
        anyhow::bail!("failed to redirect stdout to stderr");
    }
    let mut writer = BinaryWriter::new(stdout_fd);

    match Worker::load(&config) {
        Ok(worker) => serve(worker, &mut writer),
        Err(error) => {
            let _ = writer.write_frame(WORKER_OUTPUT_ERROR, 0, error.to_string().as_bytes());
            Err(error)
        }
    }
}

/// One-shot synthesis to a WAV file (no frame protocol).
pub fn generate_to_wav(config: &WorkerConfig, text: &str, output: &Path) -> Result<()> {
    let worker = Worker::load(config)?;
    let (samples, sample_rate) = worker.inference.generate_with_icl(
        text,
        &config.ref_text,
        &worker.ref_codes,
        &worker.speaker_embedding,
        &config.language,
        config.temperature,
        config.top_k,
        config.max_new_tokens,
    )?;
    let samples = if sample_rate == config.output_sample_rate {
        samples
    } else {
        resample(&samples, sample_rate, config.output_sample_rate)?
    };
    write_wav_file(path_str(output)?, &samples, config.output_sample_rate)?;
    Ok(())
}

fn serve(worker: Worker, writer: &mut BinaryWriter) -> Result<()> {
    let (sender, receiver) = mpsc::channel::<Frame>();
    let cancelled = Arc::new(Mutex::new(HashSet::<u32>::new()));
    let reader_cancelled = Arc::clone(&cancelled);

    thread::spawn(move || {
        let mut stdin = io::stdin().lock();
        loop {
            let frame = match read_frame(&mut stdin) {
                Ok(Some(frame)) => frame,
                Ok(None) => shutdown_frame(),
                Err(error) => {
                    eprintln!("worker frame read error: {error}");
                    shutdown_frame()
                }
            };
            if frame.frame_type == WORKER_INPUT_CANCEL {
                if let Ok(mut set) = reader_cancelled.lock() {
                    set.insert(frame.request_id);
                }
                continue;
            }
            let shutdown = frame.frame_type == WORKER_INPUT_SHUTDOWN;
            if sender.send(frame).is_err() || shutdown {
                break;
            }
        }
    });

    writer.write_frame(WORKER_OUTPUT_READY, 0, &[])?;
    log_json(json!({
        "type": "server_ready",
        "backend": "rust_mlx",
        "model": worker.model_name
    }));

    while let Ok(frame) = receiver.recv() {
        match frame.frame_type {
            WORKER_INPUT_SHUTDOWN => break,
            WORKER_INPUT_SPEAK => {
                if let Err(error) =
                    worker.handle_speak(frame.request_id, &frame.payload, writer, &cancelled)
                {
                    writer.write_frame(
                        WORKER_OUTPUT_ERROR,
                        frame.request_id,
                        error.to_string().as_bytes(),
                    )?;
                    clear_cancelled(&cancelled, frame.request_id);
                }
            }
            other => {
                writer.write_frame(
                    WORKER_OUTPUT_ERROR,
                    frame.request_id,
                    format!("unknown frame type {other}").as_bytes(),
                )?;
            }
        }
    }
    Ok(())
}

fn shutdown_frame() -> Frame {
    Frame {
        frame_type: WORKER_INPUT_SHUTDOWN,
        request_id: 0,
        payload: Vec::new(),
    }
}

fn read_frame<R: Read>(reader: &mut R) -> Result<Option<Frame>> {
    let mut header = [0u8; FRAME_HEADER_BYTES];
    if !read_exact(reader, &mut header)? {
        return Ok(None);
    }
    let frame_type = header[0];
    let request_id = u32::from_le_bytes(header[1..5].try_into()?);
    let payload_len = u32::from_le_bytes(header[5..9].try_into()?) as usize;
    let mut payload = vec![0u8; payload_len];
    if payload_len > 0 && !read_exact(reader, &mut payload)? {
        return Ok(None);
    }
    Ok(Some(Frame {
        frame_type,
        request_id,
        payload,
    }))
}

fn read_exact<R: Read>(reader: &mut R, buffer: &mut [u8]) -> Result<bool> {
    let mut offset = 0;
    while offset < buffer.len() {
        let read = reader.read(&mut buffer[offset..])?;
        if read == 0 {
            return Ok(false);
        }
        offset += read;
    }
    Ok(true)
}

fn path_str(path: &Path) -> Result<&str> {
    path.to_str()
        .ok_or_else(|| anyhow::anyhow!("path is not valid UTF-8: {}", path.display()))
}

fn samples_to_i16(samples: &[f32]) -> Vec<i16> {
    samples
        .iter()
        .map(|sample| (sample.clamp(-1.0, 1.0) * 32768.0).clamp(-32768.0, 32767.0) as i16)
        .collect()
}

fn i16_bytes(samples: &[i16]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(samples.len() * 2);
    for sample in samples {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    bytes
}

fn take_cancelled(cancelled: &Arc<Mutex<HashSet<u32>>>, request_id: u32) -> bool {
    cancelled
        .lock()
        .map(|mut set| set.remove(&request_id))
        .unwrap_or(false)
}

fn clear_cancelled(cancelled: &Arc<Mutex<HashSet<u32>>>, request_id: u32) {
    if let Ok(mut set) = cancelled.lock() {
        set.remove(&request_id);
    }
}

/// Normalize language aliases (`de` -> `german`); `auto` passes through.
pub fn normalize_language(language: &str) -> String {
    match language.trim().to_lowercase().replace('_', "-").as_str() {
        "" | "auto" => "auto".to_string(),
        "de" | "de-de" => "german".to_string(),
        "en" | "en-us" | "en-gb" => "english".to_string(),
        "fr" | "fr-fr" => "french".to_string(),
        "es" | "es-es" => "spanish".to_string(),
        "it" | "it-it" => "italian".to_string(),
        "pt" | "pt-br" | "pt-pt" => "portuguese".to_string(),
        "ja" | "ja-jp" => "japanese".to_string(),
        "ko" | "ko-kr" => "korean".to_string(),
        "zh" | "zh-cn" | "zh-tw" => "chinese".to_string(),
        "ru" | "ru-ru" => "russian".to_string(),
        normalized => normalized.to_string(),
    }
}

fn log_json(value: serde_json::Value) {
    eprintln!("{value}");
}

/// Whether a finished generation should be reported as failed.
///
/// `CapReached` means no EOS, so the utterance is cut off. A long silent tail
/// means the model stopped speaking but kept emitting frames. Both produce a
/// WAV that looks fine to the caller.
fn degenerate_reason(stop: crate::inference::Stop, trailing_s: f64) -> Option<String> {
    const MAX_TRAILING_S: f64 = 3.0;
    if stop.truncated() {
        return Some(
            "generation hit the frame cap without reaching EOS; the utterance is truncated \
             — split the text into shorter segments"
                .to_string(),
        );
    }
    if trailing_s > MAX_TRAILING_S {
        return Some(format!(
            "generation collapsed: {trailing_s:.1}s of trailing non-speech — split the text \
             into shorter segments"
        ));
    }
    None
}

fn clear_mlx_cache() {
    #[cfg(feature = "mlx")]
    unsafe {
        crate::backend::mlx::ffi::mlx_clear_cache();
    }
}

fn round3(value: f64) -> f64 {
    (value * 1000.0).round() / 1000.0
}
