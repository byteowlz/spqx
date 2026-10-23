#!/usr/bin/env bash
# Resolve one acceleration choice into shell exports for the justfile recipes.
#
# This never selects an engine at runtime: it only decides which Cargo features
# and native build variables a developer command uses. An explicitly requested
# accelerator that this target cannot support fails instead of quietly building
# something else. Named GGML targets may be cross-built without a local device;
# this reports that limitation, never proves inference availability. Untested accelerators (Vulkan, GGML Metal) are only
# produced when named explicitly; automatic selection stays on MLX/CPU/CUDA.
#
# Callers run `eval "$(bash scripts/accel-env.sh ACCEL)"`, so stdout must be pure
# shell assignments and every diagnostic goes to stderr. Each failure exits before
# the first export, so a failed run emits nothing to eval.
#
# Usage: eval "$(bash scripts/accel-env.sh [auto|mlx|cpu|cuda|vulkan|metal|tch])"
set -euo pipefail

requested=${1:-${SPQX_ACCEL:-auto}}
os=$(uname -s)
arch=$(uname -m)

apple_silicon=0
[[ $os == Darwin && $arch == arm64 ]] && apple_silicon=1

nvidia_present=0
if command -v nvidia-smi >/dev/null 2>&1 && nvidia-smi -L >/dev/null 2>&1; then
    nvidia_present=1
fi

if [[ $requested == auto ]]; then
    if ((apple_silicon)); then accel=mlx
    elif ((nvidia_present)); then accel=cuda
    else accel=cpu
    fi
else
    accel=$requested
fi

# Separate strings: spqx-core has no playback feature and the CLI has no
# tch-backend feature, so a plain member-package install cannot share one list.
case "$accel" in
    mlx)
        ((apple_silicon)) || {
            printf 'mlx is Apple Silicon only (this host is %s/%s); use cpu, cuda or tch\n' "$os" "$arch" >&2
            exit 2
        }
        [[ -f mlx-c/CMakeLists.txt ]] || {
            printf 'mlx-c submodule is not initialized; run: git submodule update --init --recursive\n' >&2
            exit 2
        }
        core_features=mlx,qwentts
        cli_features=mlx,qwentts,playback
        workspace_features=mlx,qwentts,playback
        ;;
    cpu | cuda | vulkan | metal)
        core_features=qwentts
        cli_features=qwentts,playback
        workspace_features=qwentts,playback
        ;;
    tch)
        [[ -n ${LIBTORCH:-} ]] || {
            printf 'legacy tch needs LIBTORCH pointing at a libtorch 2.7.x install\n' >&2
            exit 2
        }
        core_features=tch-backend
        cli_features=tch
        workspace_features=spqx-core/tch-backend,spqx-cli/tch
        ;;
    *)
        printf 'unknown accelerator: %s (expected auto, mlx, cpu, cuda, vulkan, metal or tch)\n' "$accel" >&2
        exit 2
        ;;
esac

printf 'export SPQX_ACCEL=%q\n' "$accel"
printf 'export SPQX_CORE_FEATURES=%q\n' "$core_features"
printf 'export SPQX_CLI_FEATURES=%q\n' "$cli_features"
printf 'export SPQX_FEATURES=%q\n' "$workspace_features"

# Building for an accelerator this host cannot run is legitimate (cross-building a
# CUDA runtime needs the toolkit, not the device), so these only warn.
case "$accel" in
    cuda)
        ((nvidia_present)) || printf 'note: no NVIDIA device detected; a CUDA build works here, running CUDA inference does not\n' >&2
        ;;
    vulkan | metal)
        printf 'note: %s is not a qualified spqx target; only for explicit local development\n' "$accel" >&2
        ;;
esac

# GGML is the only engine with a device choice; MLX stays on Metal/auto.
case "$accel" in
    cpu) printf 'export SPQX_DEVICE=%q\n' cpu ;;
    cuda) printf 'export SPQX_DEVICE=%q\n' cuda:0 ;;
    vulkan) printf 'export SPQX_DEVICE=%q\n' vulkan:0 ;;
esac

if ((nvidia_present)); then
    # Query the capability directly; card-name matching mis-detects anything
    # newer than the names it knows. Multiple devices build for all of them.
    capabilities=$(nvidia-smi --query-gpu=compute_cap --format=csv,noheader 2>/dev/null |
        tr -d ' .' | sort -u | paste -sd';' - || true)
    if [[ -z $capabilities ]]; then
        capabilities=$(nvidia-smi --query-gpu=name --format=csv,noheader 2>/dev/null | head -1 || true)
        case "$capabilities" in
            *"RTX 40"*) capabilities=89 ;;
            *"RTX 30"*) capabilities=86 ;;
            *"RTX 20"* | *"GTX 16"*) capabilities=75 ;;
            *) capabilities= ;;
        esac
    fi
    if [[ -n $capabilities ]]; then
        # The release workflows pin their own architectures; never override them.
        : "${SPQX_CUDA_ARCHITECTURES:=${CMAKE_CUDA_ARCHITECTURES:-$capabilities}}"
        printf 'export SPQX_CUDA_ARCHITECTURES=%q\n' "$SPQX_CUDA_ARCHITECTURES"
        printf 'export CMAKE_CUDA_ARCHITECTURES=%q\n' "$SPQX_CUDA_ARCHITECTURES"
    fi
fi

# Arch Linux keeps the toolkit outside the default prefix.
if [[ $os == Linux && -d /opt/cuda ]]; then
    printf 'export CUDA_ROOT=%q\n' /opt/cuda
    printf 'export PATH=%q\n' "/opt/cuda/bin:$PATH"
fi
