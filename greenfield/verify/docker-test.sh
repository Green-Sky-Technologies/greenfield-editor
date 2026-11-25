#!/usr/bin/env bash
# Run greenfield/test.sh in the pinned build image. A named volume keeps the cargo cache and
# target dir between runs. Usage: greenfield/verify/docker-test.sh
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ROOT="$(cd "$HERE/.." && pwd)"
# shellcheck disable=SC1091 # versions.env: plain KEY=value pins
. "$HERE/versions.env"
docker run --rm --name editor-tests \
  -v "$ROOT:/src:ro" \
  -v greenfield-editor-cargo:/usr/local/cargo/registry \
  -v greenfield-editor-target:/work/target \
  -e CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-4}" -e CARGO_TARGET_DIR=/work/target \
  "$BUILD_IMAGE" bash -euo pipefail -c '
    git config --global --add safe.directory /src
    git -C /src ls-files -z --cached --others --exclude-standard | tar -C /src --null -T - -cf - | { mkdir /work_repo && tar -xf - -C /work_repo; }
    cd /work_repo && greenfield/test.sh
  '
