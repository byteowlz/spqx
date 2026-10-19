#!/usr/bin/env bash
# Build only native runtime code, never download models or alter services.
set -euo pipefail
backend=${1:-cpu}
out=${2:-target/spqx-runtime}
root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
revision=5916bf7c79dbb4e66fc059cdc507a79a23a28bb4
ggml_revision=cc5df49a6b878db2b68b9a7b45976e279594627d
source_dir=${SPQX_QWENTTS_SOURCE:-$root/target/qwentts-source}
case "$backend" in cpu|cuda|vulkan|metal) ;; *) printf 'Expected cpu, cuda, vulkan or metal\n' >&2; exit 2;; esac
for program in git cmake; do command -v "$program" >/dev/null || { printf 'Missing %s\n' "$program" >&2; exit 2; }; done
mkdir -p "$root/target"
if [[ ! -d "$source_dir" ]]; then
    git clone --no-checkout https://github.com/ServeurpersoCom/qwentts.cpp.git "$source_dir"
    git -C "$source_dir" checkout --detach "$revision"
    git -C "$source_dir" submodule update --init --recursive
fi
# Do not reset, switch, or repair an existing checkout owned by somebody else.
[[ $(git -C "$source_dir" rev-parse HEAD) == "$revision" ]] || { printf 'Source revision mismatch\n' >&2; exit 2; }
[[ $(git -C "$source_dir/ggml" rev-parse HEAD) == "$ggml_revision" ]] || { printf 'GGML revision mismatch\n' >&2; exit 2; }
git -C "$source_dir" diff --quiet
git -C "$source_dir" diff --cached --quiet
git -C "$source_dir/ggml" diff --quiet
git -C "$source_dir/ggml" diff --cached --quiet
if [[ -d "$out" && -n $(ls -A -- "$out") ]]; then
    printf 'Use a fresh output directory; existing runtime artifacts are never mixed\n' >&2; exit 2
fi
mkdir -p "$out"
out=$(cd -- "$out" && pwd)
build="$out/build"
flags=(-DQWEN_SHARED=ON -DBUILD_SHARED_LIBS=ON -DGGML_BACKEND_DL=OFF
    -DGGML_CUDA=OFF -DGGML_VULKAN=OFF -DGGML_METAL=OFF -DGGML_NATIVE=OFF
    -DCMAKE_BUILD_TYPE=Release -DCMAKE_BUILD_WITH_INSTALL_RPATH=ON)
case "$backend" in
    cuda) flags+=(-DGGML_CUDA=ON);;
    vulkan) flags+=(-DGGML_VULKAN=ON);;
    metal) flags+=(-DGGML_METAL=ON);;
esac
if [[ $(uname -s) == Darwin ]]; then
    flags+=('-DCMAKE_INSTALL_RPATH=@loader_path')
else
    flags+=('-DCMAKE_INSTALL_RPATH=$ORIGIN')
fi
cmake -S "$source_dir" -B "$build" "${flags[@]}"
cmake --build "$build" --config Release --target qwen --parallel "${SPQX_BUILD_JOBS:-2}"
mkdir -p "$out/lib" "$out/licenses"
shopt -s nullglob
libraries=("$build"/*.so "$build"/*.so.* "$build"/*.dylib "$build"/*.dll "$build"/Release/*.dll)
((${#libraries[@]})) || { printf 'No runtime libraries produced\n' >&2; exit 2; }
for library in "${libraries[@]}"; do cp -P -- "$library" "$out/lib/"; done
cp "$source_dir/LICENSE" "$out/licenses/qwentts.LICENSE"
cp "$source_dir/ggml/LICENSE" "$out/licenses/ggml.LICENSE"
printf 'qwentts=%s\nggml=%s\nbackend=%s\n' "$revision" "$ggml_revision" "$backend" > "$out/source.txt"
printf 'Runtime libraries: %s/lib\nNot a platform qualification or redistribution-license clearance.\n' "$out"
