# spqx Development

Read `CONTEXT.md` and `docs/adr/` before changing engine boundaries. Current CLI/worker and target evidence are documented in `docs/portable-engines.md`.

## Engines

- MLX on Apple Silicon; its effective build cfg is `spqx_mlx`, emitted only for macOS/aarch64.
- Whole-pipeline GGML through pinned libqwen ABI 6 on supported CPU/CUDA targets. Do not implement another per-tensor bridge or silently substitute devices/cloud services.
- Legacy dense tch remains opt-in. Native tensor APIs are distinct from the shared prepared synthesis session.

## Features

`legacy-api` owns HTTP/multi-format encoding dependencies, including LAME. It is included in default core-library builds for compatibility; `legacy-tools` enables it. Portable release builds use `--no-default-features` and explicit `mlx,qwentts,playback` or `qwentts,playback`, so neither libtorch nor LAME is compiled into those distributions. Preserve that distinction in license inventories.

## Commands

- `just check-portable test-portable`: model-free contract/config/stdout tests; no inference.
- `just test-native-cpu`: ignored acceptance with explicit local runtime, weights and lawful references. Read its environment contract before running.
- Metal model tests require fresh accepted compute admission. Never stop unrelated workloads or edit service/launchd configuration to make room.
- `uv run --no-project scripts/test_package.py`: bundle integrity tests.

## Delivery

A Rust executable alone is not a GGML bundle. Include native dependencies, source/license inventories, checksums, install instructions and relocation proof. MLX needs its exact compiled `mlx.metallib` beside the executables; do not rely on a build-directory path. Models and reference recordings are separate user-provided assets.

Never merge unrelated tracker migration changes or generated artifacts. Track work with trx. Keep `CHANGELOG.md` current. Historical FFI/kernel notes are retained in `docs/native-tensor-notes.md`.
