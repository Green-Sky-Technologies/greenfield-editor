#!/usr/bin/env bash
# Build the Greenfield editor (this repo's web app) with pinned fonts and tools, run the marks
# check, and enforce the size budget. Every build runs the size gate; there is no way to skip it.
#
# Usage: greenfield/build.sh
#
# Pins live in versions.env. Needs on PATH: git, cargo + rustc (with the wasm32-unknown-unknown
# target), trunk, wasm-bindgen and wasm-opt at the pinned versions (toolchain.sh installs them in
# the build image; verify/docker-build.sh runs everything in that image).
#
# Environment:
#   CRAFT_FONTS_SRC   craft-fonts checkout dir, default <repo>/.cache/craft-fonts
#   FIGCRAFT_OUT      where the finished static site is copied, default <repo>/.cache/site
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/.." && pwd)"
# shellcheck disable=SC1091 # versions.env: plain KEY=value pins
. "$HERE/versions.env"
FONTS="${CRAFT_FONTS_SRC:-$ROOT/.cache/craft-fonts}"
OUT="${FIGCRAFT_OUT:-$ROOT/.cache/site}"

for tool in git cargo rustc trunk wasm-bindgen wasm-opt; do
  command -v "$tool" >/dev/null || { echo "error: $tool not found on PATH" >&2; exit 1; }
done
# Different tool versions give different bytes: refuse anything but the pins.
wasm-bindgen --version | grep -qx "wasm-bindgen $WASM_BINDGEN_VERSION" \
  || { echo "error: need wasm-bindgen $WASM_BINDGEN_VERSION, have '$(wasm-bindgen --version)'" >&2; exit 1; }
wasm-opt --version | grep -Eq "^wasm-opt version $BINARYEN_VERSION( |$)" \
  || { echo "error: need wasm-opt $BINARYEN_VERSION, have '$(wasm-opt --version)'" >&2; exit 1; }

# 1. craft-fonts at its pinned commit, pristine. Init in place rather than clone: CI restores a
#    cache before this runs, so the dir can exist without .git.
if [[ ! -d "$FONTS/.git" ]]; then
  git init --quiet "$FONTS"
  git -C "$FONTS" remote add origin "$CRAFT_FONTS_REPO"
fi
git -C "$FONTS" cat-file -e "$CRAFT_FONTS_SHA^{commit}" 2>/dev/null || git -C "$FONTS" fetch --quiet --depth 1 origin "$CRAFT_FONTS_SHA"
git -C "$FONTS" checkout --quiet --force "$CRAFT_FONTS_SHA"
git -C "$FONTS" clean -fdq
[[ "$(git -C "$FONTS" rev-parse HEAD)" == "$CRAFT_FONTS_SHA" ]] || { echo "error: $FONTS is not at $CRAFT_FONTS_SHA" >&2; exit 1; }

# 2. The marks check on the source (Story 2.2).
"$HERE/check-marks.sh" src "$ROOT"

# 3. Upstream's web recipe (packaging/web/package.sh): trunk build --release in apps/figcraft-web,
#    with craft-fonts embedded as in upstream's release builds. index.html selects the web-release
#    profile and wasm-opt -Oz. Fixed build metadata keeps the output byte-for-byte reproducible.
export CRAFT_FONTS_DIR="$FONTS" CRAFT_FONTS_REQUIRED=1
export FIGCRAFT_BUILD_SHA="$UPSTREAM_SHA"
export FIGCRAFT_BUILD_DATE="${FIGCRAFT_BUILD_DATE:-2026-01-01}"
export SOURCE_DATE_EPOCH="${SOURCE_DATE_EPOCH:-1767225600}"
start=$(date +%s)
# Fetch first (this also fetches the pinned storytold/egui and storytold/winit forks): trunk
# --offline keeps cargo off the network.
cargo fetch --manifest-path "$ROOT/Cargo.toml" --locked --target wasm32-unknown-unknown
mid=$(date +%s)
(cd "$ROOT/apps/figcraft-web" && trunk build --release --offline --locked)
end=$(date +%s)
echo "cargo fetch: $((mid - start)) s, trunk build: $((end - mid)) s"

SITE="$ROOT/dist/web"
[[ -f "$SITE/index.html" ]] || { echo "error: $SITE/index.html missing" >&2; exit 1; }

sha256() {
  if command -v sha256sum >/dev/null; then sha256sum "$1" | cut -d' ' -f1; else shasum -a 256 "$1" | cut -d' ' -f1; fi
}

# 4. Size gate (every build) and report.
count=0
for wasm in "$SITE"/*.wasm; do
  [[ -f "$wasm" ]] || continue
  count=$((count + 1))
  size=$(wc -c <"$wasm" | tr -d ' ')
  gz=$(gzip -9 -n -c "$wasm" | wc -c | tr -d ' ')
  br=""
  command -v brotli >/dev/null && br=$(brotli -q 11 -c "$wasm" | wc -c | tr -d ' ')
  echo "$(basename "$wasm"): raw $size B, gzip-9 $gz B, brotli-11 ${br:-n/a} B, sha256 $(sha256 "$wasm") (limit $WASM_MAX_BYTES B)"
  if [[ "$size" -gt "$WASM_MAX_BYTES" ]]; then
    echo "error: $(basename "$wasm") is $size bytes, over the $WASM_MAX_BYTES-byte budget (SPEC.md D26)" >&2
    exit 1
  fi
done
[[ "$count" -gt 0 ]] || { echo "error: no .wasm in $SITE" >&2; exit 1; }

# 4b. Marks check on the built site (page, glue, wasm, icons), then its negative controls.
"$HERE/check-marks.sh" site "$SITE"
"$HERE/check-marks.sh" self-test "$ROOT" "$SITE"

# 5. Site plus licences: FigCraft's MIT/Apache texts and NOTICE, and every craft-fonts family's
#    OFL text as OFL-<family-dir>.txt (upstream's copy_font_licences in packaging/env.sh), then the
#    chrome fonts' OFL texts (IBM Plex, Cormorant Garamond; Story 2.1).
rm -rf "$OUT"
mkdir -p "$OUT"
cp -R "$SITE/." "$OUT/"
for f in LICENSE-MIT LICENSE-APACHE NOTICE ATTRIBUTION.md; do cp "$ROOT/$f" "$OUT/"; done
for lic in "$FONTS"/fonts/*/OFL.txt; do
  cp "$lic" "$OUT/OFL-$(basename "$(dirname "$lic")").txt"
done
for lic in "$ROOT"/assets/fonts/OFL-*.txt; do cp "$lic" "$OUT/"; done
sums="$(cd "$OUT" && find . -type f | LC_ALL=C sort | while read -r f; do echo "$(sha256 "$f")  $f"; done)"
echo "$sums" >"$OUT/SHA256SUMS"
echo "dist: $(find "$OUT" -type f | wc -l | tr -d ' ') files, $(du -sk "$OUT" | cut -f1) KiB, SHA256SUMS sha256 $(sha256 "$OUT/SHA256SUMS")"
echo "site copied to $OUT"
