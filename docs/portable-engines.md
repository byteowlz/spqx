# Portable engines: first integration

One CLI and worker contract, different whole-pipeline engines. Apple Silicon keeps MLX; GGML supplies a libtorch-free path for other targets. Browser/mobile inference is not part of this change.

## Current evidence

- Apple Silicon: default CLI/core `cargo check` passed. Existing MLX reference preparation moved behind a shared session; no new MLX model inference was claimed for this change.
- Linux x86-64: portable-only Rust build and model-free tests passed. Real GGML CUDA English/German generation, active cancellation, queued-request fencing, recovery after malformed input, CLI JSON/WAV output, and rejection of an unavailable device passed with synthetic references. Explicit CPU inference completed using both the CUDA-capable runtime and a separately built, relocated CPU-only runtime with no CUDA/libtorch dependencies.
- Windows: CRT binary mode, Win32 stdout-handle synchronization, native DLL search policy and a model-free CI matrix are implemented. The workflow has not yet run here; Windows native inference/bundles are **not qualified**.
- Vulkan, GGML Metal, Linux ARM64 and Intel Mac inference remain untested. A device name or upstream build option is not deployment proof.

Generated speech has not received an independent listening/ASR quality review. Cancellation acknowledgement is not audible playback cessation. These checks are integration evidence, not a latency benchmark or full adoption approval.

## Build

```sh
just check-portable
just test-portable
just build-portable  # no MLX, Python, libtorch or playback dependency
```

Default builds enable MLX only on Apple Silicon and GGML elsewhere. Live playback is a separate `playback` feature (enabled by default); Linux builds with it need ALSA development files. Headless/server builds above do not. C/C++ build tools are still required by existing audio dependencies.

The tensor-level diagnostic binaries/examples now require `legacy-tools` plus the actual `mlx` or `tch-backend` feature. Rust consumers of the dense tensor API on Linux should explicitly select `default-features = false, features = ["tch-backend"]`; it is no longer the default portable engine. Existing `WorkerConfig` and `run_stdio` callers remain source-compatible; new callers can pass `EngineOptions` to `run_stdio_with_engine`.

### Native runtime

GGML uses qwentts.cpp `5916bf7c79dbb4e66fc059cdc507a79a23a28bb4`, GGML `cc5df49a6b878db2b68b9a7b45976e279594627d`, public ABI 6. Source identity is checked before using layout-sensitive functions. Different native builds require explicit qualification and a binding update, not disabling the check.

```sh
just runtime cpu target/spqx-runtime-cpu
# Optional build targets; these require their own SDK and device validation:
# just runtime cuda target/spqx-runtime-cuda
# just runtime vulkan target/spqx-runtime-vulkan
```

The Bash helper clones pinned source only when needed, never downloads models, never changes services, and refuses mixed/reused output directories. `SPQX_QWENTTS_SOURCE` may name an already initialized clean pinned checkout; `SPQX_BUILD_JOBS` defaults to 2. Keep all generated native libraries together in `lib/`, along with source/license metadata. This is a developer build helper, not a finished Windows installer or redistribution-license clearance. Audit all bundled code, weights and audio dependencies separately before publishing packages.

Default library discovery checks only the executable directory and its `lib/` or `runtime/` subdirectory. Otherwise select a trusted library explicitly. Windows additionally limits dependency lookup to safe DLL search directories. Native code is not sandboxed.

## Assets and configuration

The tested Base model pair from `Serveurperso/Qwen3-TTS-GGUF` revision `b7ee2e8c7459c3bea99da23e3d178125a7d1713c` is:

- `qwen-talker-0.6b-base-Q8_0.gguf`, 992,615,488 bytes; SHA256 `d54dbaf10591421fa764ed630d764efa717ae40cd959bd48c66d4eb1af226426`.
- `qwen-tokenizer-12hz-F32.gguf`, 647,263,104 bytes; SHA256 `b16b95557c7c7340a121757bd6855b9609e1cf4ad3fad0778b89393293ae5f3d`.

Obtain only the chosen files, not the entire multi-variant repository. Model assets are local and separate from the native runtime. An explicit GGUF path is recommended; a directory with the tested Q8/F32 filenames also works.

```toml
[model]
path = "/models/qwen-talker-0.6b-base-Q8_0.gguf"
[engine]
backend = "ggml"
device = "cpu" # or cuda:0 / vulkan:0 on a validated runtime/device
library = "/opt/spqx/lib/libqwen.so"
codec = "/models/qwen-tokenizer-12hz-F32.gguf"
[voice]
language = "german"
```

`spqx config schema` describes the new engine section. Flags override environment (`SPQX_BACKEND`, `SPQX_DEVICE`, `SPQX_ENGINE_LIBRARY`, `SPQX_CODEC`), then config/defaults. Automatic engine selection uses model format and compiled availability; it never downloads a model or silently maps an unavailable explicitly requested device to another accelerator/cloud service.

```sh
spqx --backend ggml --device cpu say "Der Bericht ist fertig." \
  --ref-audio reference.wav --ref-text-file reference.txt --no-play --out result.wav
spqx --backend ggml --device cuda:0 serve \
  --ref-audio reference.wav --ref-text-file reference.txt
```

Use a lawful, same-language reference with its matching transcript. Conditioning remains checkpoint-scoped. Registry identity does not make it cross-engine enrollment or give turn/session authority.

Standalone `spqx-tts-worker` accepts `--backend`, `--device`, `--engine-library`, and `--codec`; `--device` is now honored, defaults to `auto`, and is no longer an ignored Python-compatibility flag. MLX accepts auto/Metal; explicit CUDA requests on MLX fail. Other legacy compatibility flags, including `--seed`, retain their previous limitations.

## Streaming bounds and remaining work

The 9-byte frame protocol, request IDs, mono PCM16 and error/done frames are unchanged. GGML prepares reference latents once and polls cancellation on its compute thread. The native GGML chunk ramp differs from MLX's fixed frame policy. GGML frame-cap detection is conservative: reaching the cap is an error even if EOS coincides exactly with it; the pinned ABI does not return a distinct EOS reason. CLI generation now uses the same 1,536-frame ceiling as the worker and fails on truncation rather than claiming completion.

Remaining gates: release-qualified CPU-only/CUDA/Vulkan runtime bundles, Windows native execution and binary framing, actual MLX regression synthesis, platform installers, dependency/license manifests, failure-injection coverage and speech-quality review. Track engine integration in `spqx-srtg.1`, packaging in `spqx-z9ng`, and runtime adoption in `spqx-wzrp`.
