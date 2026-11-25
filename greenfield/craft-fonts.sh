#!/usr/bin/env bash
# Check out craft-fonts at its pinned commit (versions.env), pristine. Used by build.sh and
# build-cli.sh. Init in place rather than clone: CI restores a cache before this runs, so the dir
# can exist without .git.
# Usage: greenfield/craft-fonts.sh <dir>
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091 # versions.env: plain KEY=value pins
. "$HERE/versions.env"
FONTS="$1"
if [[ ! -d "$FONTS/.git" ]]; then
  git init --quiet "$FONTS"
  git -C "$FONTS" remote add origin "$CRAFT_FONTS_REPO"
fi
git -C "$FONTS" cat-file -e "$CRAFT_FONTS_SHA^{commit}" 2>/dev/null || git -C "$FONTS" fetch --quiet --depth 1 origin "$CRAFT_FONTS_SHA"
git -C "$FONTS" checkout --quiet --force "$CRAFT_FONTS_SHA"
git -C "$FONTS" clean -fdq
[[ "$(git -C "$FONTS" rev-parse HEAD)" == "$CRAFT_FONTS_SHA" ]] || { echo "error: $FONTS is not at $CRAFT_FONTS_SHA" >&2; exit 1; }
