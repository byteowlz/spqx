//! Opt-in real CPU/DLL/stdio acceptance. No downloads or inference by default.
//! Set SPQX_TEST_ASSETS to a prepared models/, references/, runtime/lib/ tree.
use anyhow::{bail, ensure, Context, Result};
use std::{
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc::{self, Receiver},
    thread,
    time::{Duration, Instant},
};

struct Frame {
    kind: u8,
    id: u32,
    data: Vec<u8>,
}
struct Worker {
    child: Child,
    input: ChildStdin,
    frames: Receiver<Result<Frame, String>>,
}
fn read_frame(input: &mut impl Read) -> Result<Frame> {
    let mut header = [0; 9];
    input.read_exact(&mut header)?;
    let size = u32::from_le_bytes(header[5..9].try_into()?) as usize;
    ensure!(size <= 1024 * 1024, "oversized frame");
    let mut data = vec![0; size];
    input.read_exact(&mut data)?;
    Ok(Frame {
        kind: header[0],
        id: u32::from_le_bytes(header[1..5].try_into()?),
        data,
    })
}
fn worker_command(root: &Path, backend: &str) -> Result<Command> {
    let library = if cfg!(windows) {
        "qwen.dll"
    } else if cfg!(target_os = "macos") {
        "libqwen.dylib"
    } else {
        "libqwen.so"
    };
    let worker = std::env::var_os("SPQX_TEST_WORKER")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_spqx-tts-worker")));
    let mut command = Command::new(worker);
    command.args([
        "--serve",
        "--backend",
        backend,
        "--device",
        if backend == "mlx" { "auto" } else { "cpu" },
    ]);
    let model = if backend == "mlx" {
        PathBuf::from(std::env::var_os("SPQX_TEST_MODEL").context("set SPQX_TEST_MODEL for MLX")?)
    } else {
        let selected = std::env::var_os("SPQX_TEST_LIBRARY");
        if selected.as_deref() != Some(std::ffi::OsStr::new("auto")) {
            command.arg("--engine-library").arg(
                selected
                    .map(PathBuf::from)
                    .unwrap_or_else(|| root.join("runtime/lib").join(library)),
            );
        }
        command
            .arg("--codec")
            .arg(root.join("models/qwen-tokenizer-12hz-F32.gguf"));
        root.join("models/qwen-talker-0.6b-base-Q8_0.gguf")
    };
    command.arg("--model-path").arg(model);
    Ok(command)
}

impl Worker {
    fn start(root: &Path, language: &str, output: &Path, backend: &str) -> Result<Self> {
        let mut command = worker_command(root, backend)?;
        let mut child = command
            .arg("--ref-audio")
            .arg(root.join(format!("references/{language}.wav")))
            .arg("--ref-text-file")
            .arg(root.join(format!("references/{language}.txt")))
            .args([
                "--language",
                language,
                "--output-sample-rate",
                "24000",
                "--max-new-tokens",
                "128",
            ])
            .current_dir(output)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(File::create(output.join(format!("{language}-worker.log")))?)
            .spawn()?;
        let input = child.stdin.take().context("worker stdin")?;
        let mut stdout = child.stdout.take().context("worker stdout")?;
        let (sender, frames) = mpsc::channel();
        // Windows selectors cannot wait on pipe handles. A bounded frame reader
        // thread works on every target and exits on child EOF or receiver drop.
        thread::spawn(move || loop {
            let result = read_frame(&mut stdout).map_err(|e| e.to_string());
            let done = result.is_err();
            if sender.send(result).is_err() || done {
                break;
            }
        });
        let worker = Self {
            child,
            input,
            frames,
        };
        let ready = worker.frame(Instant::now() + Duration::from_secs(180))?;
        ensure!(
            ready.kind == 1 && ready.id == 0 && ready.data.is_empty(),
            "not a clean ready frame: {} {} {}",
            ready.kind,
            ready.id,
            String::from_utf8_lossy(&ready.data)
        );
        Ok(worker)
    }
    fn frame(&self, deadline: Instant) -> Result<Frame> {
        self.frames
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))?
            .map_err(anyhow::Error::msg)
    }
    fn send(&mut self, kind: u8, id: u32, payload: &[u8]) -> Result<()> {
        self.input.write_all(&[kind])?;
        self.input.write_all(&id.to_le_bytes())?;
        self.input
            .write_all(&u32::try_from(payload.len())?.to_le_bytes())?;
        self.input.write_all(payload)?;
        self.input.flush()?;
        Ok(())
    }
    fn speak(&mut self, id: u32, text: &str, wav: &Path) -> Result<()> {
        self.send(1, id, text.as_bytes())?;
        let mut audio = Audio::default();
        let deadline = Instant::now() + Duration::from_secs(180);
        loop {
            let frame = self.frame(deadline)?;
            ensure!(frame.id == id, "stale request frame");
            if audio.accept(frame)? {
                break;
            }
        }
        audio.save(wav)
    }
    fn cancel_and_recover(&mut self, output: &Path) -> Result<()> {
        self.send(1, 2, b"This is a deliberately long synthetic sentence to test interruption while the worker is actively producing speech. Please keep reading until cancellation arrives.")?;
        let deadline = Instant::now() + Duration::from_secs(180);
        loop {
            let frame = self.frame(deadline)?;
            ensure!(frame.id == 2, "wrong active request");
            ensure!(
                frame.kind != 4 && frame.kind != 5,
                "request ended before cancellation"
            );
            if frame.kind == 3 && !frame.data.is_empty() {
                break;
            }
        }
        let cancelled = Instant::now();
        self.send(2, 2, &[])?;
        self.send(1, 3, b"The worker can speak again.")?;
        let mut terminal = false;
        let mut audio = Audio::default();
        loop {
            let frame = self.frame(deadline)?;
            if frame.id == 2 {
                ensure!(
                    !terminal && frame.kind != 5,
                    "stale output or error after cancellation"
                );
                if frame.kind == 4 {
                    ensure!(frame.data.is_empty(), "invalid cancellation terminal");
                    terminal = true;
                    println!(
                        "cancel_ack_ms={:.1}",
                        cancelled.elapsed().as_secs_f64() * 1000.0
                    );
                }
            } else {
                ensure!(
                    frame.id == 3 && terminal,
                    "queued request crossed cancellation fence"
                );
                if audio.accept(frame)? {
                    break;
                }
            }
        }
        audio.save(&output.join("recovery.wav"))?;
        self.send(1, 4, &[0xff])?;
        let bad = self.frame(Instant::now() + Duration::from_secs(10))?;
        ensure!(
            bad.kind == 5 && bad.id == 4 && !bad.data.is_empty(),
            "malformed input not rejected"
        );
        self.speak(
            5,
            "The connection still works.",
            &output.join("after-error.wav"),
        )
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.send(3, 0, &[]);
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if matches!(self.child.try_wait(), Ok(Some(_))) {
                return;
            }
            thread::sleep(Duration::from_millis(20));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
#[derive(Default)]
struct Audio {
    started: bool,
    bytes: Vec<u8>,
}
impl Audio {
    fn accept(&mut self, frame: Frame) -> Result<bool> {
        match frame.kind {
            2 => {
                ensure!(
                    !self.started && frame.data == 24000u32.to_le_bytes(),
                    "bad audio_start"
                );
                self.started = true;
            }
            3 => {
                ensure!(
                    self.started && frame.data.len().is_multiple_of(2),
                    "bad PCM framing"
                );
                ensure!(
                    self.bytes.len() + frame.data.len() <= 24000 * 2 * 20,
                    "excessive output"
                );
                self.bytes.extend(frame.data);
            }
            4 => {
                ensure!(
                    frame.data.is_empty() && !self.bytes.is_empty(),
                    "empty or invalid terminal"
                );
                return Ok(true);
            }
            5 => bail!("worker error: {}", String::from_utf8_lossy(&frame.data)),
            _ => bail!("unexpected frame type {}", frame.kind),
        }
        Ok(false)
    }
    fn save(self, path: &Path) -> Result<()> {
        let samples: Vec<f32> = self
            .bytes
            .chunks_exact(2)
            .map(|v| i16::from_le_bytes([v[0], v[1]]) as f32 / 32768.0)
            .collect();
        ensure!(samples.iter().any(|v| v.abs() > 0.001), "silent output");
        println!("pcm_seconds={:.3}", samples.len() as f64 / 24000.0);
        spqx_core::audio::write_wav_file(
            path.to_str().context("UTF-8 output path")?,
            &samples,
            24000,
        )?;
        Ok(())
    }
}

#[test]
#[ignore = "requires explicitly prepared local CPU runtime, weights and lawful references"]
fn cpu_runtime_worker_contract() -> Result<()> {
    exercise_contract("ggml")
}

#[cfg(all(target_os = "macos", target_arch = "aarch64", feature = "mlx"))]
#[test]
#[ignore = "explicit Metal admission and SPQX_TEST_MODEL required"]
fn mlx_runtime_worker_contract() -> Result<()> {
    exercise_contract("mlx")
}

fn exercise_contract(backend: &str) -> Result<()> {
    let root = PathBuf::from(std::env::var_os("SPQX_TEST_ASSETS").context("set SPQX_TEST_ASSETS")?);
    let temporary = tempfile::tempdir()?;
    let output = std::env::var_os("SPQX_TEST_OUTPUT")
        .map(PathBuf::from)
        .unwrap_or_else(|| temporary.path().join("output"));
    std::fs::create_dir(&output).context("use a fresh SPQX_TEST_OUTPUT directory")?;
    for (language, text) in [
        ("en", "The speech engine is ready."),
        ("de", "Die Sprachausgabe ist bereit."),
    ] {
        let mut worker = Worker::start(&root, language, &output, backend)?;
        worker.speak(1, text, &output.join(format!("{language}.wav")))?;
        if language == "en" {
            worker.cancel_and_recover(&output)?;
        }
    }
    Ok(())
}
