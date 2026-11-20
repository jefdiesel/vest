#!/bin/bash
# stage.sh <out-dir>: buildable copies of the two Basecamp packages, from this checkout.
#   <out>/module  the `vesting` core module: flake + rust-lib/, with the repository crates it
#                 uses copied into rust-lib/vest/ (the builder compiles only rust-lib/)
#   <out>/app     the `vesting_ui` mini-app (QML), whose `vesting` input is <out>/module
# Then, on a machine with nix (see docs/BASECAMP-TEST.md):
#   (cd <out>/module && nix build .#lgx-portable)
#   (cd <out>/app && nix build .#lgx-portable --override-input vesting path:<out>/module)
set -euo pipefail
R=$(cd "$(dirname "$0")/../.." && pwd)
O=${1:?give an output folder}
case "$O" in /*) ;; *) O="$PWD/$O" ;; esac
rm -rf "$O"; mkdir -p "$O/module/rust-lib/vest/miniapp/module" "$O/app"
X=(--exclude target --exclude 'result*' --exclude generated --exclude vest --exclude logos-rust-sdk-src)
rsync -a "${X[@]}" "$R/miniapp/module/" "$O/module/" --exclude core --exclude stage.sh
for d in core sdk program vendor; do rsync -a "${X[@]}" "$R/$d" "$O/module/rust-lib/vest/"; done
rsync -a "${X[@]}" "$R/miniapp/module/core" "$O/module/rust-lib/vest/miniapp/module/"
rsync -a "${X[@]}" "$R/miniapp/" "$O/app/" --exclude module --exclude '*.test.mjs' --exclude README.md
echo "staged in $O"
