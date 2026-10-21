# Portable model-free work never allocates an accelerator.
set windows-shell := ["powershell.exe", "-NoProfile", "-Command"]

check:
    cargo check --locked -p spqx-core -p spqx-cli

check-portable:
    cargo check --locked -p spqx-core -p spqx-cli --no-default-features --features qwentts

test-portable:
    cargo test --locked -p spqx-core -p spqx-cli --no-default-features --features qwentts --lib --bins --tests

# Explicit VoiceDesign assets and fresh compute admission required; see docs/voice-design.md.
# SPQX_DESIGN_BACKEND=mlx uses SPQX_DESIGN_ASSETS as a VoiceDesign snapshot directory
# and SPQX_DESIGN_WRONG as a Base snapshot; the default GGML path keeps the GGUF layout.
test-native-design:
    cargo test --locked -p spqx-core --no-default-features --features qwentts --test voice_design -- --ignored --nocapture

test-native-design-mlx:
    cargo test --locked --release -p spqx-core --no-default-features --features mlx,qwentts --test voice_design -- --ignored --nocapture

# Explicit lawful ICL inputs, fresh output and external accepted admission; see docs/frozen-icl.md.
test-native-icl:
    cargo test --locked -p spqx-cli --no-default-features --features qwentts --test frozen_icl -- --ignored --nocapture

test-native-cpu:
    cargo test --locked -p spqx-core --no-default-features --features qwentts --test native_cpu -- --ignored --nocapture

# Run inside an x64 Visual Studio developer prompt, with CMake and Ninja on PATH.
runtime-windows-cpu source out:
    cmake -S "{{source}}" -B "{{out}}" -G Ninja -DQWEN_SHARED=ON -DBUILD_SHARED_LIBS=ON -DGGML_BACKEND_DL=OFF -DGGML_NATIVE=OFF -DGGML_CUDA=OFF -DGGML_VULKAN=OFF -DGGML_METAL=OFF -DGGML_BLAS=OFF -DGGML_OPENMP=OFF -DCMAKE_BUILD_TYPE=Release
    cmake --build "{{out}}" --target qwen --parallel 2

build-portable:
    cargo build --locked --release -p spqx-core -p spqx-cli --no-default-features --features qwentts --bins

runtime backend="cpu" out="target/spqx-runtime":
    bash scripts/build-qwentts-runtime.sh "{{backend}}" "{{out}}"

test-package:
    uv run --no-project scripts/test_package.py

bundle target backend:
    bash scripts/build-bundle.sh "{{target}}" "{{backend}}"

fmt:
    cargo fmt -p spqx-core -p spqx-cli

quality:
    ripwire crates --quality-delta --legend=compact
