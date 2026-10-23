#!/usr/bin/env bash
# Copy the exact compiled mlx.metallib from the MLX build tree beside executables.
#
# MLX loads its kernel library from the directory of the binary that contains it
# (dladdr on the MLX object), so dlopen-ing or symlinking the binaries from
# elsewhere changes where the metallib is looked up. The copied file must be the
# one this MLX source revision compiles to: a metallib from another MLX revision,
# a Python wheel or the other Cargo profile is not interchangeable.
#
# The profile is explicit. Multiple outputs are usable only if all their bytes
# agree; otherwise require SPQX_METALLIB to name the intended build artifact.
# Never guess compatibility from mtime. Keep generation in MLX's own build.
#
# Usage: stage-metallib.sh DEST_DIR [release|debug]
set -euo pipefail

dest=${1:?usage: stage-metallib.sh DEST_DIR [release|debug]}
profile=${2:-release}
build=${CARGO_TARGET_DIR:-target}

[[ -d $dest ]] || { printf 'no such destination directory: %s\n' "$dest" >&2; exit 2; }
case "$profile" in release|debug) ;; *) printf 'unknown profile: %s (expected release or debug)\n' "$profile" >&2; exit 2;; esac

shopt -s nullglob
if [[ -n ${SPQX_METALLIB:-} ]]; then
    candidates=("$SPQX_METALLIB")
else
    candidates=("$build/$profile"/build/spqx-core-*/out/lib/mlx.metallib)
fi
((${#candidates[@]})) || {
    printf 'no compiled mlx.metallib for the %s profile under %s; build MLX first:\n  just build mlx\n' \
        "$profile" "$build" >&2
    exit 2
}
source_metallib=${candidates[0]}
[[ -f $source_metallib ]] || { printf 'no such compiled metallib: %s\n' "$source_metallib" >&2; exit 2; }
for candidate in "${candidates[@]}"; do
    if ! cmp -s "$source_metallib" "$candidate"; then
        printf 'incompatible compiled metallibs; set SPQX_METALLIB to the intended build artifact\n' >&2
        exit 2
    fi
done

if [[ ! "$source_metallib" -ef "$dest/mlx.metallib" ]]; then
    cp -f -- "$source_metallib" "$dest/mlx.metallib"
fi
printf 'Staged %s -> %s/mlx.metallib\n' "$source_metallib" "$dest"
printf 'sha256 %s\n' "$(shasum -a 256 "$dest/mlx.metallib" | awk '{print $1}')"
