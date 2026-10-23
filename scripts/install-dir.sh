#!/usr/bin/env bash
# Print the `cargo install` root so a justfile recipe can stage files beside the
# installed binaries. Honors CARGO_INSTALL_ROOT and CARGO_HOME like Cargo does.
set -euo pipefail

if [[ -n ${CARGO_INSTALL_ROOT:-} ]]; then
    root=$CARGO_INSTALL_ROOT
elif [[ -n ${CARGO_HOME:-} ]]; then
    root=$CARGO_HOME
else
    root=$HOME/.cargo
fi
# Relative roots are resolved from the invocation directory by Cargo as well.
printf '%s/bin\n' "$(cd -- "$root" 2>/dev/null && pwd || printf '%s' "$root")"
