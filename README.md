# spqx

Fast Qwen3-TTS inference in Rust with an Apple Silicon MLX backend: streaming,
in-context voice cloning, quantized models, and a persistent worker protocol
built for real-time voice runtimes. Built on top of Mario's fork of
[qwen3_tts_rs](https://github.com/badlogic/qwen3_tts_rs); spqx is the default
TTS engine of [foxline](https://github.com/WismutHansen/foxline).

Measured against the reference Python MLX stack (`mlx_audio` via
`speech-to-speech`) on the same machine, same model
(`Qwen3-TTS-12Hz-0.6B-Base-6bit`), same seven cloned voices, 49 utterances:

|                     | Python MLX | spqx      |
| ------------------- | ---------- | --------- |
| Time to first audio | 120 ms     | **96 ms** |
| (p95)               | 175 ms     | 152 ms    |
| Real-time factor    | 0.22       | **0.20**  |
| Model load          | 2.3 s      | **0.3 s** |

The 0.3s model load makes per-session workers and instant voice switching
practical.

## Why it is fast

- Everything decode-side runs on device with one GPU sync per audio frame:
  device-chained sub-code sampling, on-device repetition penalty, cached RoPE
  tables, fused SDPA with dtype-correct masks, bf16 end to end.
- The per-voice ICL prompt (reference text + codec frames) is precomputed once
  per session; requests only pay for their own text.
- Sampling mirrors `mlx_audio` exactly — repetition penalty 1.5 applied once
  per unique code for ICL, special-token suppression, EOS exempt from top-k —
  so long generations terminate reliably and voices match the reference
  implementation.
- Output cleanup: the vocoder streaming state is pre-warmed with the tail of
  the reference audio (ICL semantics: generated speech continues from the
  reference), and a duration-capped click gate removes utterance-start
  transients without ever touching plosives or soft leading words
  (ASR-verified).
- Numerical guardrails ship as tests: full-batch quantized matmul against a
  chunked reference, and sampling-penalty parity against the Python semantics.

## Binaries

- `spqx-tts-worker` — persistent binary-framed worker (stdin/stdout, 9-byte
  frame header). Drop-in compatible with the
  [PiBot](https://github.com/badlogic/pibot)/[foxline](https://github.com/byteowlz/foxline)
  Python worker protocol and CLI. `pibot-tts-worker` remains as an
  upstream-name alias.
- `tts` — one-shot text-to-speech with preset voices.
- `voice_clone` — one-shot voice cloning from reference audio + transcript.
- `api_server` — OpenAI-compatible HTTP speech API (early; see roadmap).
- `qwen3-tts` — timed CLI for performance work; `trace_rust` generates MLX
  parity traces against the Python implementation (compare tools in
  `scripts/`).

## Requirements

- Apple Silicon Mac for the MLX backend. A libtorch (`tch`) backend exists for
  Linux/CUDA with dense bf16 weights.
- Rust toolchain; CMake, pkg-config, Opus; Xcode command line tools with the
  Metal toolchain.

```bash
brew install cmake pkg-config opus
xcodebuild -downloadComponent MetalToolchain
```

Known toolchain hazards are handled in `build.rs`: it pins an explicit macOS
deployment target and patches an MLX `__builtin_available(macOS 26)` misfire
that otherwise breaks Metal kernel JIT on macOS 15 with the Xcode 26
toolchain.

## Build

```bash
git submodule update --init --recursive
cargo build --release -p spqx-core --no-default-features --features mlx --bin spqx-tts-worker
```

## Usage

Persistent worker (what [foxline](https://github.com/WismutHansen/foxline)'s
`rust-mlx` TTS backend launches):

```bash
target/release/spqx-tts-worker \
  --serve \
  --model-path /path/to/Qwen3-TTS-12Hz-0.6B-Base-6bit \
  --ref-audio /path/to/reference.wav \
  --ref-text-file /path/to/reference.txt \
  --language auto \
  --output-sample-rate 24000
```

The worker reads `speak`/`cancel`/`shutdown` frames on stdin and streams PCM
`audio_chunk` frames on stdout; JSON status events (`ready`, `ttfa`,
`generated`) go to stderr. `--model-path` accepts the MLX quantized snapshots
from `mlx-community` (4/6/8-bit) or dense BF16 `Qwen/Qwen3-TTS` layouts.

One-shot cloning:

```bash
target/release/voice_clone /path/to/Qwen3-TTS-12Hz-0.6B-Base-6bit \
  reference.wav "Hello from a cloned voice." english "Reference transcript"
```

### Debug/profiling knobs

- `SPQX_TIMING=1` — log per-request prefill timing (adds one GPU sync).
- `SPQX_TIMING_DETAIL=1` — per-layer-group prefill breakdown.
- `SPQX_NO_GATE=1` — bypass all output cleanup (raw model audio).
- `MLX_C_CHUNKED_QMM=1` — re-enable chunked quantized matmul (see below).

## Quantized matmul and the Metal toolchain

Some Xcode 26 Metal toolchains miscompile the locally built `mlx.metallib`,
producing incorrect results for large-M transposed 6-bit `quantized_matmul`
(upstream report: <https://github.com/ml-explore/mlx/issues/3586>, repro:
<https://github.com/badlogic/mlx-qmm-repro>). The mlx-c layer used here
([byteowlz/mlx-c](https://github.com/byteowlz/mlx-c), branch `spqx`) makes the
16-row chunking workaround for that case opt-in via `MLX_C_CHUNKED_QMM=1`
rather than always-on — unconditional chunking costs 3-4x on prompt prefill.
On a sound toolchain the full-batch kernel matches a chunked reference to bf16
noise; verify yours with:

```bash
cargo test --release --no-default-features --features mlx \
  --lib qmm_full_batch_matches_chunked_reference -- --nocapture
```

## Voice references

Cloning quality is bounded by the reference recording. Hard-won guidelines:

- 6-12 seconds; longer references measurably degrade first-word reliability.
- End on a sentence boundary with a natural pause; trailing cut-off transients
  poison both the vocoder warm state and the clone.
- Scan for isolated clicks (spike over near-silence) — the clone reproduces
  reference artifacts as voice mannerisms.
- After changing a reference, generate a round and ASR-check the first words.

An experimental wrapper can select a transcript-aligned 6-12 second clip using
[`trnscrb`](../trnscrb):

```bash
scripts/prepare_voice_reference.sh -o prepared --play recording.wav
# The script writes and plays prepared/reference.wav; then enroll it:
spqx voices add demo --ref-audio prepared/reference.wav \
  --ref-text-file prepared/reference.txt
```

The wrapper requires `trnscrb`, `jq`, `ffmpeg`, and `ffprobe`. It defaults to
the Parakeet backend, writes a mono 24 kHz WAV, and records its selection in
`preparation.json`. Pass `--transcript /path/to/transcript.json` to reuse an
existing trnscrb transcript without running transcription again. This is an
MVP: always listen to the selected clip before enrollment.

## Benchmarking

`spqx say --json` reports the core latency metrics for one synthesis run:
time to first audio (`ttfa_ms`), generated audio duration (`audio_s`), wall
clock time (`wall_s`), and real-time factor (`rtf`). Use
`scripts/benchmark_spqx.sh` to repeat runs and capture process resource metrics
from `/usr/bin/time`:

```bash
scripts/benchmark_spqx.sh --voice demo --runs 10 -- "Hello from spqx."
scripts/benchmark_spqx.sh --ref-audio ref.wav --ref-text-file ref.txt --runs 10 -- "Benchmark text."
```

The script writes `results.jsonl`, per-run WAV files, `/usr/bin/time` output,
a `summary.txt` when `jq` is installed, and a self-contained `report.html` when
`uv` is available. For a polished demo, open the report next to Finder/QuickTime
and play one of the generated WAV files while showing TTFA, RTF, and memory.
Hardware counters such as Metal GPU power require separate system tools
(`powermetrics`, Instruments, or Activity Monitor) because the CLI process
metrics only cover CPU-side resource use.

## Python/Rust parity tools

`scripts/trace_python_mlx.py` and `trace_rust` dump per-layer tensors from
both implementations; `scripts/compare_traces.py` diffs them. The trace tools
support forced/reference code paths to isolate talker/code-predictor parity
from audio-encoder drift.

## Lineage and license

spqx is a fork of
[badlogic/qwen3_tts_rs](https://github.com/badlogic/qwen3_tts_rs) by Mario
Zechner (derived from
[second-state/qwen3_tts_rs](https://github.com/second-state/qwen3_tts_rs) by
Second State), maintained by byteowlz as a standalone TTS engine. The worker
protocol and worker-based voice architecture originate from Mario's
[PiBot](https://github.com/badlogic/pibot); spqx serves as the TTS engine of
[foxline](https://github.com/byteowlz/foxline), byteowlz's local voice
gateway. The model is Qwen3-TTS by the Alibaba Qwen team. Improvements that
are not byteowlz-specific are offered back upstream. Apache-2.0.

## Roadmap

- Single `spqx` binary with subcommands (`say`, `clone`, `serve`, `api`,
  `ref check|trim|verify`) and a packaged metallib.
- Hardened OpenAI-compatible API with chunked streaming and a named-voice
  registry.
- Candle backend for pure-Rust quantized CUDA/CPU inference.

Issue tracking lives in-repo via `trx`.
