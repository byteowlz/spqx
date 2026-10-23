# spqx developer commands.
#
# Acceleration is chosen, not guessed: build/install/check/test recipes take an
# optional accelerator argument or read SPQX_ACCEL. `auto` resolves to MLX on
# Apple Silicon, CUDA when an NVIDIA device is present, otherwise CPU. An
# unavailable target configuration fails instead of quietly building another one.
# Named GGML targets may be cross-built without a local device; this never proves
# inference availability.
#
#   auto  mlx  cpu  cuda  vulkan  metal  tch
#   mlx          Apple Silicon MLX (the accelerated path on macOS)
#   cpu cuda     whole-pipeline GGML via pinned libqwen (ABI 6)
#   vulkan metal the same GGML engine on those upstream backends; untested here,
#                only produced when named explicitly
#   tch          legacy dense libtorch, opt-in, needs LIBTORCH
#
# Portable/model-free work never allocates an accelerator:
# `just check-portable test-portable` stay MLX-, CUDA- and libtorch-free.
#
# The accelerator-aware recipes need bash; on Windows x64 use
# `scripts\build-bundle.cmd` for a finished bundle or run them under Git Bash.
set positional-arguments
set windows-shell := ["powershell.exe", "-NoProfile", "-Command"]

# Built-artifact root; Cargo reads CARGO_TARGET_DIR itself, this only mirrors it
# for the recipes that look at files directly.
target_dir := env("CARGO_TARGET_DIR", "target")
default_accel := env("SPQX_ACCEL", "auto")

# Show available recipes.
default:
    @just --list

# Report the accelerator and Cargo features these recipes would select.
accel requested=default_accel:
    #!/usr/bin/env bash
    set -euo pipefail
    resolved=$(bash scripts/accel-env.sh "${1:-auto}") || exit 2
    eval "$resolved"
    printf 'accelerator: %s\ncore:        %s\ncli:         %s\nworkspace:   %s\n' \
        "$SPQX_ACCEL" "$SPQX_CORE_FEATURES" "$SPQX_CLI_FEATURES" "$SPQX_FEATURES"
    if [[ -n ${SPQX_DEVICE:-} ]]; then printf 'device:      %s\n' "$SPQX_DEVICE"; fi
    if [[ -n ${SPQX_CUDA_ARCHITECTURES:-} ]]; then printf 'cuda arch:   %s\n' "$SPQX_CUDA_ARCHITECTURES"; fi

# === Build ===

# Debug build of the selected accelerator.
build accel=default_accel:
    #!/usr/bin/env bash
    set -euo pipefail
    resolved=$(bash scripts/accel-env.sh "${1:-auto}") || exit 2
    eval "$resolved"
    cargo build --locked -p spqx-core -p spqx-cli --no-default-features --features "$SPQX_FEATURES" --bins

# Release build of the selected accelerator.
build-release accel=default_accel:
    #!/usr/bin/env bash
    set -euo pipefail
    resolved=$(bash scripts/accel-env.sh "${1:-auto}") || exit 2
    eval "$resolved"
    cargo build --locked --release -p spqx-core -p spqx-cli --no-default-features --features "$SPQX_FEATURES" --bins

# Portable GGML build without MLX, libtorch or LAME (unchanged contract).
build-portable:
    cargo build --locked --release -p spqx-core -p spqx-cli --no-default-features --features qwentts --bins

# Build one engine's native runtime from the pinned source (cpu, cuda, vulkan or metal).
# The CUDA architecture is detected for this host unless SPQX_CUDA_ARCHITECTURES is set.
runtime backend="cpu" out="target/spqx-runtime":
    #!/usr/bin/env bash
    set -euo pipefail
    resolved=$(bash scripts/accel-env.sh "${1:-cpu}") || exit 2
    eval "$resolved"
    bash scripts/build-qwentts-runtime.sh "{{backend}}" "{{out}}"

# Run inside an x64 Visual Studio developer prompt, with CMake and Ninja on PATH.
runtime-windows-cpu source out:
    cmake -S "{{source}}" -B "{{out}}" -G Ninja -DQWEN_SHARED=ON -DBUILD_SHARED_LIBS=ON -DGGML_BACKEND_DL=OFF -DGGML_NATIVE=OFF -DGGML_CUDA=OFF -DGGML_VULKAN=OFF -DGGML_METAL=OFF -DGGML_BLAS=OFF -DGGML_OPENMP=OFF -DCMAKE_BUILD_TYPE=Release
    cmake --build "{{out}}" --target qwen --parallel 2

# === Install ===

# Install the workspace binaries for the selected accelerator.
#
# This replaces already installed spqx binaries (`cargo install --force`), like the
# eaRS/byteowlz install recipes. MLX stages the exact compiled mlx.metallib beside
# the executables; GGML builds need SPQX_RUNTIME (a directory built by `just
# runtime`) for libqwen in <bin>/lib. Configuration, voices, models and recordings
# are never modified.
install accel=default_accel:
    #!/usr/bin/env bash
    set -euo pipefail
    resolved=$(bash scripts/accel-env.sh "${1:-auto}") || exit 2
    eval "$resolved"
    bin=$(bash scripts/install-dir.sh)
    # Keep build outputs for exact metallib staging after cargo install.
    export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$PWD/target}"
    runtime=${SPQX_RUNTIME:-}
    libraries=()
    if [[ $SPQX_ACCEL != mlx && $SPQX_ACCEL != tch && -n $runtime ]]; then
        [[ -d $runtime/lib ]] || { printf 'SPQX_RUNTIME has no lib/: %s\n' "$runtime" >&2; exit 2; }
        shopt -s nullglob
        libraries=("$runtime"/lib/*.so "$runtime"/lib/*.so.* "$runtime"/lib/*.dylib "$runtime"/lib/*.dll)
        ((${#libraries[@]})) || { printf 'no native libraries in %s/lib\n' "$runtime" >&2; exit 2; }
    fi
    printf 'Installing %s build into %s\n' "$SPQX_ACCEL" "$bin"
    cargo install --locked --path crates/spqx-core --force --no-default-features --features "$SPQX_CORE_FEATURES" --bins
    cargo install --locked --path crates/spqx-cli --force --no-default-features --features "$SPQX_CLI_FEATURES"
    if [[ $SPQX_ACCEL == mlx ]]; then
        # `cargo install` builds the release profile, so stage the release metallib.
        bash scripts/stage-metallib.sh "$bin" release
    elif [[ $SPQX_ACCEL != tch ]]; then
        if [[ -n $runtime ]]; then
            mkdir -p "$bin/lib"
            cp -P -- "${libraries[@]}" "$bin/lib/"
            printf 'Native engine libraries copied from %s/lib\n' "$runtime"
        else
            printf 'No SPQX_RUNTIME given: install worked, but GGML synthesis still needs\n' >&2
            printf '  just runtime %s target/spqx-runtime-%s && SPQX_RUNTIME=target/spqx-runtime-%s just install %s\n' \
                "$SPQX_ACCEL" "$SPQX_ACCEL" "$SPQX_ACCEL" "$SPQX_ACCEL" >&2
        fi
    fi
    printf 'Installed: %s/spqx, %s/spqx-tts-worker (models and voices are separate)\n' "$bin" "$bin"

# Install for Apple Silicon MLX, including the compiled metallib.
install-mlx: (install "mlx")

# Install a GGML build for an NVIDIA device; needs SPQX_RUNTIME for synthesis.
install-cuda: (install "cuda")

# Install the portable CPU build; needs SPQX_RUNTIME for synthesis.
install-cpu: (install "cpu")

# Install the legacy dense libtorch build; needs LIBTORCH set.
install-tch: (install "tch")

# Stage the exact compiled mlx.metallib next to built or installed executables.
metallib dest=(target_dir + "/release") profile="release":
    #!/usr/bin/env bash
    set -euo pipefail
    bash scripts/stage-metallib.sh "${1:?destination directory required}" "${2:-release}"

# Run the built release CLI (`just run auto say "hello"`).
run accel=default_accel *args:
    #!/usr/bin/env bash
    set -euo pipefail
    # First positional is the accelerator, not a spqx argument.
    resolved=$(bash scripts/accel-env.sh "${1:-auto}") || exit 2
    eval "$resolved"
    shift || true
    exec "{{target_dir}}/release/spqx" "$@"

# === Code quality ===

# Check the workspace with its default features.
check:
    cargo check --locked -p spqx-core -p spqx-cli

# Check the selected accelerator's feature set.
check-selected accel=default_accel:
    #!/usr/bin/env bash
    set -euo pipefail
    resolved=$(bash scripts/accel-env.sh "${1:-auto}") || exit 2
    eval "$resolved"
    cargo check --locked -p spqx-core -p spqx-cli --no-default-features --features "$SPQX_FEATURES" --all-targets

# Model-free portable check: no MLX, libtorch or LAME.
check-portable:
    cargo check --locked -p spqx-core -p spqx-cli --no-default-features --features qwentts

# Lint the selected accelerator's feature set; extra arguments go to clippy.
clippy accel=default_accel *args:
    #!/usr/bin/env bash
    set -euo pipefail
    resolved=$(bash scripts/accel-env.sh "${1:-auto}") || exit 2
    eval "$resolved"
    shift || true
    cargo clippy --locked -p spqx-core -p spqx-cli --no-default-features --features "$SPQX_FEATURES" --all-targets "$@"

# Alias for clippy.
lint accel=default_accel: (clippy accel)

# Apply machine-applicable lint fixes.
fix accel=default_accel:
    #!/usr/bin/env bash
    set -euo pipefail
    resolved=$(bash scripts/accel-env.sh "${1:-auto}") || exit 2
    eval "$resolved"
    cargo clippy --fix --allow-dirty --tests -p spqx-core -p spqx-cli --no-default-features --features "$SPQX_FEATURES"

# Format the workspace.
fmt:
    cargo fmt -p spqx-core -p spqx-cli

# Verify formatting without writing.
fmt-check:
    cargo fmt -p spqx-core -p spqx-cli --check

# Build the rustdoc for the workspace.
docs:
    cargo doc --locked --no-deps -p spqx-core -p spqx-cli --open

# Structural quality delta against the configured baseline.
quality:
    ripwire crates --quality-delta --legend=compact

# === Test ===

# Run the ordinary test suite for the selected accelerator.
test accel=default_accel:
    #!/usr/bin/env bash
    set -euo pipefail
    resolved=$(bash scripts/accel-env.sh "${1:-auto}") || exit 2
    eval "$resolved"
    cargo test --locked -p spqx-core -p spqx-cli --no-default-features --features "$SPQX_FEATURES" --lib --bins --tests

# Model-free contract/config/stdout tests; no inference.
test-portable:
    cargo test --locked -p spqx-core -p spqx-cli --no-default-features --features qwentts --lib --bins --tests

# Opt-in real CPU/DLL/stdio acceptance; see docs/portable-engines.md for SPQX_TEST_ASSETS.
test-native-cpu:
    cargo test --locked -p spqx-core --no-default-features --features qwentts --test native_cpu -- --ignored --nocapture

# Opt-in VoiceDesign acceptance; needs explicit GGUF assets and compute admission.
test-native-design:
    cargo test --locked -p spqx-core --no-default-features --features qwentts --test voice_design -- --ignored --nocapture

# Opt-in MLX VoiceDesign acceptance; SPQX_DESIGN_BACKEND=mlx with a snapshot directory,
# SPQX_DESIGN_ASSETS + SPQX_DESIGN_WRONG, fresh output and compute admission.
test-native-design-mlx:
    cargo test --locked --release -p spqx-core --no-default-features --features mlx,qwentts --test voice_design -- --ignored --nocapture

# Explicit lawful ICL inputs, fresh output and external accepted admission; see docs/frozen-icl.md.
test-native-icl:
    cargo test --locked -p spqx-cli --no-default-features --features qwentts --test frozen_icl -- --ignored --nocapture

# Model-free developer-command, feature selection and metallib staging tests.
test-dev-tools:
    uv run --no-project scripts/test_dev_tools.py

# Bundle integrity, installer and manifest tests.
test-package:
    uv run --no-project scripts/test_package.py

# === Release artifacts ===

# Build a complete native bundle: `just bundle aarch64-apple-darwin mlx`.
bundle target backend:
    bash scripts/build-bundle.sh "{{target}}" "{{backend}}"

# Verify every recorded file and hash of an extracted bundle.
verify-bundle path:
    uv run --no-project scripts/package.py --verify "{{path}}"

# === Housekeeping ===

# Remove Cargo build outputs; also drops the pinned target/ native checkouts.
clean:
    cargo clean

# Remove build outputs and staged bundles.
clean-all:
    #!/usr/bin/env bash
    set -euo pipefail
    cargo clean
    rm -rf dist
