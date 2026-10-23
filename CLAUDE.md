# spqx Development

Read `CONTEXT.md` and `docs/adr/` before changing engine boundaries. Current CLI/worker and target evidence are documented in `docs/portable-engines.md`.

## Engines

- MLX on Apple Silicon; its effective build cfg is `spqx_mlx`, emitted only for macOS/aarch64.
- Whole-pipeline GGML through pinned libqwen ABI 6 on supported CPU/CUDA targets. Do not implement another per-tensor bridge or silently substitute devices/cloud services.
- Legacy dense tch remains opt-in. Native tensor APIs are distinct from the shared prepared synthesis session.

## Features

`legacy-api` owns HTTP/multi-format encoding dependencies, including LAME. It is included in default core-library builds for compatibility; `legacy-tools` enables it. Portable release builds use `--no-default-features` and explicit `mlx,qwentts,playback` or `qwentts,playback`, so neither libtorch nor LAME is compiled into those distributions. Preserve that distinction in license inventories.

## Commands

The `justfile` selects the accelerator (`auto`, `mlx`, `cpu`, `cuda`, `vulkan`,
`metal`, `tch`) via `scripts/accel-env.sh`; `auto` is MLX on Apple Silicon, CUDA
on an NVIDIA host, otherwise CPU. Unsupported target configurations fail closed;
named GGML targets can be cross-built without a local device, with a warning.
Do not interpret compilation as runtime device qualification or silently build a
different engine. Explicit recipe arguments override SPQX_ACCEL.

- `just accel [accel]`: print the accelerator and Cargo features a recipe would use.
- `just check-portable test-portable`: model-free contract/config/stdout tests; no inference.
- `just test-native-cpu`: ignored acceptance with explicit local runtime, weights and lawful references. Read its environment contract before running.
- `just install mlx`: `cargo install` for both crates plus `scripts/stage-metallib.sh`, which copies the exact compiled `mlx.metallib` beside the installed binaries. Never replace this with a bare `cargo install --features mlx`; MLX loads the metallib from its own binary directory, so that install produces a broken binary.
- `just install cpu|cuda`: needs `SPQX_RUNTIME` (from `just runtime`) to place libqwen in `<cargo-bin>/lib`.
- Metal model tests require fresh accepted compute admission. Never stop unrelated workloads or edit service/launchd configuration to make room.
- `uv run --no-project scripts/test_package.py`: bundle integrity tests.

## Delivery

A Rust executable alone is not a GGML bundle. Include native dependencies, source/license inventories, checksums, install instructions and relocation proof. MLX needs its exact compiled `mlx.metallib` beside the executables; do not rely on a build-directory path. Models and reference recordings are separate user-provided assets.

Never merge unrelated tracker migration changes or generated artifacts. Track work with trx. Keep `CHANGELOG.md` current. Historical FFI/kernel notes are retained in `docs/native-tensor-notes.md`.
