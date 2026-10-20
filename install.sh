#!/usr/bin/env bash
# Install a complete spqx bundle. Never download models/voices or change services.
set -euo pipefail
repo=${SPQX_REPO:-byteowlz/spqx}
backend=auto
version=latest
prefix=${HOME}/.local
bundle=
usage() { printf 'Usage: install.sh [--bundle ARCHIVE] [--backend cpu|cuda|mlx] [--version TAG] [--prefix DIR]\n'; }
while (($#)); do
    case "$1" in
        --bundle) bundle=$2; shift 2;;
        --backend) backend=$2; shift 2;;
        --version) version=$2; shift 2;;
        --prefix) prefix=$2; shift 2;;
        --help|-h) usage; exit 0;;
        *) usage >&2; exit 2;;
    esac
done
case "$(uname -s)/$(uname -m)" in
    Darwin/arm64) target=aarch64-apple-darwin; [[ $backend != auto ]] || backend=mlx;;
    Linux/x86_64) target=x86_64-unknown-linux-gnu; [[ $backend != auto ]] || backend=cpu;;
    *) printf 'No validated prebuilt target for this platform; see docs/portable-engines.md. Windows bundles include install.cmd.\n' >&2; exit 2;;
esac
case "$target/$backend" in
    aarch64-apple-darwin/mlx|x86_64-unknown-linux-gnu/cpu|x86_64-unknown-linux-gnu/cuda) ;;
    *) printf 'No validated runtime for requested target/backend\n' >&2; exit 2;;
esac
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
if [[ -z $bundle ]]; then
    asset="spqx-$target-$backend.tar.gz"
    if [[ $version == latest ]]; then base="https://github.com/$repo/releases/latest/download";
    else base="https://github.com/$repo/releases/download/$version"; fi
    curl --fail --location --silent --show-error "$base/$asset" -o "$work/$asset"
    curl --fail --location --silent --show-error "$base/$asset.sha256" -o "$work/$asset.sha256"
    bundle="$work/$asset"
fi
[[ -f $bundle && -f $bundle.sha256 ]] || { printf 'Bundle and adjacent .sha256 file are required\n' >&2; exit 2; }
expected=$(awk 'NR==1 {print $1}' "$bundle.sha256")
[[ $expected =~ ^[a-fA-F0-9]{64}$ ]] || { printf 'Invalid checksum file\n' >&2; exit 2; }
if command -v sha256sum >/dev/null; then actual=$(sha256sum "$bundle" | awk '{print $1}');
else actual=$(shasum -a 256 "$bundle" | awk '{print $1}'); fi
[[ $actual == "$expected" ]] || { printf 'Bundle checksum mismatch\n' >&2; exit 2; }
# Reject absolute/parent member paths before extracting the verified archive.
while IFS= read -r member; do
    case "$member" in /*|../*|*/../*|*/..) printf 'Unsafe archive path\n' >&2; exit 2;; esac
done < <(tar -tzf "$bundle")
mkdir "$work/unpacked"
tar -xzf "$bundle" -C "$work/unpacked"
shopt -s nullglob
roots=("$work/unpacked"/*)
[[ ${#roots[@]} == 1 && -f ${roots[0]}/install.sh ]] || { printf 'Not a complete spqx bundle\n' >&2; exit 2; }
bash "${roots[0]}/install.sh" "$prefix"
printf 'Models and lawful references are configured separately; no existing configuration was changed.\n'
