#!/usr/bin/env bash
# Native Unix CI/developer bundle build. No model downloads or service changes.
set -euo pipefail
target=${1:?target triple required}
backend=${2:?cpu, cuda or mlx required}
root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd "$root"
export CARGO_BUILD_JOBS=${CARGO_BUILD_JOBS:-2}
export CMAKE_BUILD_PARALLEL_LEVEL=${CMAKE_BUILD_PARALLEL_LEVEL:-2}
features=qwentts,playback
[[ $backend != mlx ]] || features=mlx,qwentts,playback
mkdir -p target dist/release
cargo build --locked --release --no-default-features --features "$features" --bins --message-format=json > target/bundle-build.jsonl
cargo fetch --locked --target "$target"
args=(--target "$target" --backend "$backend" --features "$features" --binaries target/release --output dist/release)
if [[ $backend == mlx ]]; then
    metallib=$(uv run --no-project python - <<'PY'
import json
from pathlib import Path
for line in Path('target/bundle-build.jsonl').read_text().splitlines():
 try: item=json.loads(line)
 except ValueError: continue
 if item.get('reason')=='build-script-executed' and 'spqx-core' in item.get('package_id',''):
  path=Path(item['out_dir'])/'lib/mlx.metallib'
  if path.is_file(): print(path);break
else: raise SystemExit('exact compiled MLX metallib not found')
PY
)
    uv run --no-project scripts/native_materials.py target/release/build target/mlx-materials
    args+=(--native-source mlx-c --metallib "$metallib" --requirement 'macOS 15+, Apple Silicon, Metal device')
    for source in target/mlx-materials/*; do args+=(--material "$source"); done
else
    runtime="target/bundle-runtime-$backend"
    bash scripts/build-qwentts-runtime.sh "$backend" "$runtime"
    args+=(--runtime "$runtime/lib" --native-source "${SPQX_QWENTTS_SOURCE:-$root/target/qwentts-source}" --requirement 'Linux x86-64 with AVX2, glibc and ALSA runtime')
    if [[ $backend == cuda ]]; then
        cuda=${CUDA_HOME:-/usr/local/cuda}
        for lib in libcudart.so.12 libcublas.so.12 libcublasLt.so.12; do args+=(--extra-library "$cuda/targets/x86_64-linux/lib/$lib"); done
        if [[ -f "$cuda/EULA.txt" ]]; then eula="$cuda/EULA.txt";
        elif [[ -f "$cuda/doc/EULA.txt" ]]; then eula="$cuda/doc/EULA.txt";
        else
            # Minimal NVIDIA development images omit documentation packages.
            # Never omit terms or silently substitute another toolkit version.
            eula=target/CUDA-EULA.txt
            uv run --no-project scripts/vendor_notices.py --cuda "${SPQX_CUDA_EULA_VERSION:?set the exact CUDA EULA documentation version}" "$eula"
        fi
        args+=(--extra-license "$eula" --requirement 'NVIDIA CUDA 12-compatible system driver; no driver files bundled')
    fi
fi
uv run --no-project scripts/package.py "${args[@]}"
