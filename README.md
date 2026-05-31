# qwen3-tts-rs

Rust Qwen3-TTS inference with an Apple Silicon MLX backend. This fork is maintained for Pipi's native TTS worker and for Rust/Python MLX parity work.

## What this fork contains

- `pibot-tts-worker`: persistent binary-framed TTS worker used by Pipi.
- `tts`: one-shot text-to-speech CLI.
- `voice_clone`: one-shot voice-cloning CLI.
- `api_server`: OpenAI-compatible HTTP speech API.
- `trace_rust`: Rust trace generator for MLX parity debugging.
- Python trace/compare tools in `scripts/`.

The important path for Pipi is the MLX backend plus `pibot-tts-worker`.

## Requirements

- Apple Silicon Mac.
- Rust toolchain.
- Xcode command line tools and Metal toolchain.
- CMake, pkg-config, and Opus.

```bash
brew install cmake pkg-config opus
xcodebuild -downloadComponent MetalToolchain
```

## Build

From this repository:

```bash
git submodule update --init --recursive
cargo build --release --no-default-features --features mlx --bin pibot-tts-worker
```

Build all MLX binaries:

```bash
cargo build --release --no-default-features --features mlx
```

Pipi builds this submodule with:

```bash
npm run build:tts-rust
```

from the Pipi repository root.

## Models

Pipi currently defaults to the 0.6B Base 6-bit MLX model:

```text
mlx-community/Qwen3-TTS-12Hz-0.6B-Base-6bit
```

The 1.7B Base 6-bit MLX model is also supported and was used for parity/performance testing:

```text
mlx-community/Qwen3-TTS-12Hz-1.7B-Base-6bit
```

Dense BF16 Qwen/Qwen3-TTS models are still supported. Dense MLX linear weights and biases are intentionally cast to FP32 in this fork; this is required for good audio quality with the unquantized 0.6B model.

## Pipi worker

Example worker invocation:

```bash
target/release/pibot-tts-worker \
  --serve \
  --model-name /path/to/qwen3-tts-model \
  --ref-audio /path/to/reference.wav \
  --ref-text-file /path/to/reference.txt \
  --language de \
  --output-sample-rate 24000 \
  --temperature 0.7 \
  --top-k 30
```

The worker uses a binary stdin/stdout protocol and logs status/events on stderr. It emits streamed PCM chunks for low-latency playback.

Language names and short aliases are normalized by the worker, e.g. `de` maps to `german`. `auto` remains supported.

## One-shot CLIs

Text-to-speech with a preset/custom voice model:

```bash
target/release/tts /path/to/Qwen3-TTS-12Hz-0.6B-CustomVoice \
  "Hello world" Vivian english
```

Voice cloning with a Base model:

```bash
target/release/voice_clone /path/to/Qwen3-TTS-12Hz-0.6B-Base \
  reference.wav \
  "Hello from a cloned voice." \
  english \
  "Transcript of the reference audio"
```

Reference audio should be mono 24 kHz WAV.

## Python/Rust MLX parity tools

Generate Python MLX traces:

```bash
python3 scripts/trace_python_mlx.py \
  --model /path/to/model \
  --ref-audio /path/to/reference.wav \
  --ref-text-file /path/to/reference.txt \
  --text "Hallo" \
  --language de \
  --out /tmp/python-trace
```

Generate Rust MLX traces:

```bash
cargo run --release --no-default-features --features mlx --bin trace_rust -- \
  --model /path/to/model \
  --ref-audio /path/to/reference.wav \
  --ref-text-file /path/to/reference.txt \
  --text "Hallo" \
  --language de \
  --out /tmp/rust-trace
```

Compare traces:

```bash
python3 scripts/compare_traces.py /tmp/python-trace /tmp/rust-trace
```

The trace tools support forced/reference code paths used to isolate talker/code-predictor parity from audio-encoder drift.

## MLX qmatmul workaround

MLX loads `mlx.metallib` at runtime for Metal kernels. With Xcode 26.5 / Metal compiler `metalfe-32023.883`, locally source-built MLX `v0.31.2` can produce incorrect results for large-M transposed 6-bit `quantized_matmul` / `qmm_splitk` calls. The official Python MLX wheel artifact does not show the same issue.

The vendored MLX-C wrapper in this fork works around the bad local Metal artifact by chunking large 2D/3D transposed quantized matmul calls into 16-token slices before calling MLX core. This keeps MLX's real quantized Metal kernels, avoids dequantization, and restores parity with Python MLX for Qwen3-TTS.

Standalone repro:

```text
https://github.com/badlogic/mlx-qmm-repro
```

Upstream issue:

```text
https://github.com/ml-explore/mlx/issues/3586
```

## License

Apache-2.0

## Credits

Based on the original Rust implementation from Second State and the Qwen3-TTS Python/MLX implementation from the Alibaba Qwen team.
