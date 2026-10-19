# Portable model-free work never allocates an accelerator.
set windows-shell := ["powershell.exe", "-NoProfile", "-Command"]

check:
    cargo check --locked -p spqx-core -p spqx-cli

check-portable:
    cargo check --locked -p spqx-core -p spqx-cli --no-default-features --features qwentts

test-portable:
    cargo test --locked -p spqx-core -p spqx-cli --no-default-features --features qwentts --lib --bins --tests

build-portable:
    cargo build --locked --release -p spqx-core -p spqx-cli --no-default-features --features qwentts --bins

runtime backend="cpu" out="target/spqx-runtime":
    bash scripts/build-qwentts-runtime.sh "{{backend}}" "{{out}}"

fmt:
    cargo fmt -p spqx-core -p spqx-cli

quality:
    ripwire crates --quality-delta --legend=compact
