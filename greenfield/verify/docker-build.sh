#!/usr/bin/env bash
# Clean-runner check: toolchain.sh + build.sh in a fresh container from the pinned image (the one
# the CI job uses), with nothing cached. Output lands in .cache/dist-docker-<name>/.
# Usage: greenfield/verify/docker-build.sh [run-name]   (CARGO_BUILD_JOBS, default 8, bounds memory;
# the profile fixes codegen units, so it doesn't change the output)
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ROOT="$(cd "$HERE/.." && pwd)"
# shellcheck disable=SC1091 # versions.env: plain KEY=value pins
. "$HERE/versions.env"
NAME="${1:-1}"
OUT="$ROOT/.cache/dist-docker-$NAME"
rm -rf "$OUT" && mkdir -p "$OUT"
# The repo is copied into the container so the build leaves no target/ or dist/ in your checkout.
docker run --rm --name "editor-build-$NAME" \
  -v "$ROOT:/src:ro" \
  -v "$OUT:/out" \
  -e CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-8}" -e CRAFT_FONTS_SRC=/work/craft-fonts -e FIGCRAFT_OUT=/out/dist \
  "$BUILD_IMAGE" bash -euo pipefail -c '
    t0=$(date +%s); git config --global --add safe.directory /src
    git -C /src ls-files -z --cached --others --exclude-standard | tar -C /src --null -T - -cf - | { mkdir /work_repo && tar -xf - -C /work_repo; }
    cd /work_repo
    greenfield/toolchain.sh
    t1=$(date +%s); echo "toolchain setup: $((t1 - t0)) s"
    greenfield/build.sh
    t2=$(date +%s); echo "build.sh: $((t2 - t1)) s; total: $((t2 - t0)) s"
  ' 2>&1 | tee "$OUT/build.log"
