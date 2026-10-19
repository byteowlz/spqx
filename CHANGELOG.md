# Changelog

## Unreleased

- Add a shared prepared synthesis session and a pinned GGML/libqwen whole-pipeline engine behind the CLI and existing worker protocol; keep MLX on Apple Silicon.
- Add explicit engine/device/codec/runtime-library config and flags, fail closed for unavailable requested devices, and cache GGML reference conditioning across requests.
- Make portable-only builds independent of MLX/libtorch/playback. Restrict effective MLX compilation to Apple Silicon; add cross-platform CRT framing and Windows stdout-handle synchronization.
- Honor cancellation of native final partial audio chunks, including after EOS; check late cancellation before flushing worker buffers.
- Bound CLI synthesis to 1,536 frames, propagate resampling failures, and treat truncated output as failure. Legacy tensor diagnostics/examples now require `legacy-tools` with their native engine feature.
- Add model-free platform CI, protocol/config/stdout regressions, a pinned native-runtime build helper, and qualification notes in `docs/portable-engines.md`. Linux CUDA/CPU integration checks passed; Windows/runtime release bundles are not yet qualified.

Migration: standalone `--device` now defaults to `auto` and is honored rather than ignored. Existing MLX deployments that explicitly passed a CUDA compatibility value must select auto/Metal instead. Published release packaging remains macOS-only pending portable bundle qualification.
