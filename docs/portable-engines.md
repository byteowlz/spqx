# Portable engines

One CLI and worker contract, different whole-pipeline engines. Apple Silicon keeps MLX; GGML supplies a libtorch-free path for other targets. Browser/mobile inference is not part of this change.

## Current evidence

- Apple Silicon: actual integrated release-worker MLX English/German synthesis, cancellation, queued fencing and malformed-input recovery passed on an M4 Max. A second run passed with copied binaries/metallib and the entire original build tree hidden. Each model run used fresh admitted capacity; it was released afterward.
- Linux x86-64: portable-only Rust build and model-free tests passed. Real GGML CUDA English/German generation, active cancellation, queued-request fencing, recovery after malformed input, CLI JSON/WAV output, and rejection of an unavailable device passed with synthetic references. Explicit CPU inference completed using both the CUDA-capable runtime and a separately built, relocated CPU-only runtime with no CUDA/libtorch dependencies.
- Windows x86-64: MSVC portable CLI/worker and all 23 model-free tests passed on a CPU-only Windows 11 VM (8 vCPUs, 16 GiB RAM). The pinned AVX2 CPU DLLs built, actual English/German CLI JSON/WAV and worker PCM generation passed, as did cancellation, queued-request fencing and malformed-input recovery. A repeatable opt-in native test passed. GPU execution, ARM64, playback and distributable bundles remain unqualified; the hosted workflow itself has not been run.
- Packaged Linux CUDA12 SM86/89 candidate: actual RTX3070 (SM86) English/German CLI-worker generation, cancellation, queued fencing and malformed-input recovery passed. First `spqx serve` use fetched both pinned GGUFs into an empty cache before readiness. Observed own peak was 3,084 MiB under a fresh 4 GiB device lease, released afterward; the unrelated recognizer remained running. This is functional integration proof, not a new matched performance benchmark.
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

### Native Windows CPU build and acceptance

Use an x64 Visual Studio developer command prompt with CMake and Ninja on PATH. No execution-policy or service changes are required. Prepare a clean qwentts.cpp checkout at the exact source/submodule revisions above, then:

```text
just runtime-windows-cpu C:/src/qwentts.cpp C:/build/qwen-cpu
```

Keep `qwen.dll`, `ggml.dll`, `ggml-base.dll` and `ggml-cpu.dll` together. The tested build disables OpenMP/BLAS and uses AVX2; `GGML_NATIVE=OFF` does not by itself establish compatibility with older non-AVX2 CPUs. MSVC runtime dependency/redistribution requirements remain part of release qualification.

For the opt-in end-to-end test, prepare an absolute `SPQX_TEST_ASSETS` directory with `models/` containing the tested pair below, `references/{en,de}.{wav,txt}` containing lawful synthetic references and matching UTF-8 transcripts, and `runtime/lib/` containing the native CPU libraries. `SPQX_TEST_OUTPUT` optionally names a fresh output directory for WAVs/logs.

```text
just test-native-cpu
```

The test uses a native pipe-reader thread rather than POSIX-only pipe selectors. It enforces frame bounds, readiness, 24 kHz PCM, request IDs, cancellation/recovery and owned-child cleanup. No download or model inference occurs in the default model-free suite. This test is integration evidence, not broad language-quality or audible-playback verification. The observed VM CLI runs were slower than real time; cancellation terminal timing is not audio-device cessation.

## Complete runtime bundles

`bash scripts/build-bundle.sh <target> cpu|cuda|mlx` and `scripts\\build-bundle.cmd` (Windows x64 CPU) build explicit lean release features. They omit `legacy-api` and therefore legacy HTTP/LAME/Opus encoders and libtorch. Default core-library builds retain `legacy-api` for compatibility.

Bundles contain the CLI/worker, matching native libraries or MLX metallib, installers, per-file hashes, original Rust crate sources/notices, native corresponding sources and vendor terms. CUDA includes unmodified CUDA runtime/cuBLAS redistributables, not drivers. Windows includes app-local release CRT DLLs from the licensed VS redistribution directory, not debug/runtime utility files. Windows release producers must have appropriate Visual Studio redistribution rights; end-user runtime terms alone are not a distributor grant. Source/build tooling is not a legal clearance service.

`uv run --no-project scripts/package.py --verify <extracted-root>` checks the recorded files. `install.sh --bundle <archive>` verifies its adjacent `.sha256` and installs without replacing existing commands/configuration. Extracted Windows bundles use `install.cmd [destination]`. No installer changes services, PATH, execution policy, model files or reference recordings. A Linux wrapper sets only its process-local library path; do not run the `-real` executable directly. Use a fresh destination for side-by-side upgrades.

The release workflow builds macOS ARM64 MLX, Linux x64 CPU/CUDA12-SM86+SM89 and Windows x64 CPU candidates with dependency/source inventories. It does not provision model weights or execute GPU inference. OS frameworks, Linux glibc/libstdc++/ALSA, Windows UCRT and NVIDIA drivers remain system dependencies. Deployment minimums are determined by the actual build host/ELF symbols, not the target triple alone. The existing Windows proof is a native VM/developer-toolchain environment, not a fresh Windows installation.

For relocated acceptance, the opt-in test supports `SPQX_TEST_WORKER=<absolute copied worker>` and `SPQX_TEST_LIBRARY=auto` (GGML executable-relative discovery). On Apple Silicon, build `native_cpu` with `--features mlx,qwentts` and select the ignored `mlx_runtime_worker_contract` test, setting `SPQX_TEST_MODEL` to an explicit prepared MLX snapshot. Model execution requires fresh compute admission independently of compilation/packaging.

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

`spqx config schema` describes the new engine section. Flags override environment (`SPQX_BACKEND`, `SPQX_DEVICE`, `SPQX_ENGINE_LIBRARY`, `SPQX_CODEC`), then config/defaults. Automatic engine selection uses model format and compiled availability. On first use, the CLI now downloads only its compatible pinned model profile when missing; it never silently maps an unavailable explicitly requested device to another accelerator/cloud service. Use `spqx models download` to prefetch, or `--offline` / `SPQX_OFFLINE=true` for cache-only operation. `HF_HUB_OFFLINE=1` also forces offline. Set `[model] auto_download=false` to disable implicit fetches. Explicit `model.path` is never overwritten; custom model IDs remain local-cache-only. See ADR 0002 for cache/integrity semantics.

```sh
spqx --backend ggml --device cpu say "Der Bericht ist fertig." \
  --ref-audio reference.wav --ref-text-file reference.txt --no-play --out result.wav
spqx --backend ggml --device cuda:0 serve \
  --ref-audio reference.wav --ref-text-file reference.txt
```

For description-only generation with a separate VoiceDesign checkpoint, see [native VoiceDesign](voice-design.md). This adds no worker opcode and does not change the reference-cloning mode below; MLX description-only generation is implemented separately in the same document.

Use a lawful, same-language reference with its matching transcript. Conditioning remains checkpoint-scoped. Registry identity does not make it cross-engine enrollment or give turn/session authority.

Standalone `spqx-tts-worker` accepts `--backend`, `--device`, `--engine-library`, and `--codec`; `--device` is now honored, defaults to `auto`, and is no longer an ignored Python-compatibility flag. MLX accepts auto/Metal; explicit CUDA requests on MLX fail. Other legacy compatibility flags, including `--seed`, retain their previous limitations.

## Streaming bounds and remaining work

The 9-byte frame protocol, request IDs, mono PCM16 and error/done frames are unchanged. GGML prepares reference latents once and polls cancellation on its compute thread. The native GGML chunk ramp differs from MLX's fixed frame policy. GGML frame-cap detection is conservative: reaching the cap is an error even if EOS coincides exactly with it; the pinned ABI does not return a distinct EOS reason. CLI generation now uses the same 1,536-frame ceiling as the worker and fails on truncation rather than claiming completion.

Additional adoption gates: Vulkan/GGML Metal, Windows GPU/ARM64, non-AVX2 CPUs, independent speech-quality review and physical playback. Hosted builds establish compilation/integrity, not model/device qualification. Do not represent these as tested targets. Track engine integration in `spqx-srtg.1`, packaging in `spqx-z9ng`, and runtime adoption in `spqx-wzrp`.
