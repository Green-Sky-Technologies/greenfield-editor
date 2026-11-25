#!/usr/bin/env bash
# Build the headless figcraft-cli for Linux x86_64 (Vercel Sandbox's platform; Story 4.5 jobs),
# statically linked against musl so it runs on any x86_64 Linux whatever its glibc (the Sandbox is
# Amazon Linux 2023, glibc 2.34, older than the build image's). craft-fonts is embedded, as in
# upstream's releases, so text lays out the same on a machine with no fonts installed.
#
# Usage: greenfield/build-cli.sh
#
# Needs, on an x86_64 host: git, cargo with the x86_64-unknown-linux-musl target, musl-gcc
# (Debian's musl-tools) and binutils' readelf. Pins live in versions.env.
#
# Environment:
#   CRAFT_FONTS_SRC   craft-fonts checkout dir, default <repo>/.cache/craft-fonts
#   CLI_OUT           where figcraft-cli-linux-x86_64.tar.gz is written, default <repo>/.cache/cli
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/.." && pwd)"
# shellcheck disable=SC1091 # versions.env: plain KEY=value pins
. "$HERE/versions.env"
FONTS="${CRAFT_FONTS_SRC:-$ROOT/.cache/craft-fonts}"
OUT="${CLI_OUT:-$ROOT/.cache/cli}"
TARGET=x86_64-unknown-linux-musl

[[ "$(uname -m)" == x86_64 ]] || { echo "error: build on x86_64 (the smoke test runs the binary)" >&2; exit 1; }
for tool in git cargo musl-gcc readelf; do
  command -v "$tool" >/dev/null || { echo "error: $tool not found on PATH" >&2; exit 1; }
done

"$HERE/craft-fonts.sh" "$FONTS"

export CRAFT_FONTS_DIR="$FONTS" CRAFT_FONTS_REQUIRED=1
export FIGCRAFT_BUILD_SHA="$UPSTREAM_SHA"
export FIGCRAFT_BUILD_DATE="${FIGCRAFT_BUILD_DATE:-2026-01-01}"
export SOURCE_DATE_EPOCH="${SOURCE_DATE_EPOCH:-1767225600}"
export CC_x86_64_unknown_linux_musl=musl-gcc
export CARGO_PROFILE_RELEASE_STRIP=symbols
start=$(date +%s)
cargo build --manifest-path "$ROOT/Cargo.toml" --release --locked -p figcraft-cli --target "$TARGET"
echo "cargo build: $(($(date +%s) - start)) s"
BIN="${CARGO_TARGET_DIR:-$ROOT/target}/$TARGET/release/figcraft-cli"

# Static: no program interpreter, no shared libraries.
if readelf -l "$BIN" | grep -q INTERP || readelf -d "$BIN" | grep -q NEEDED; then
  echo "error: $BIN is not statically linked" >&2
  readelf -d "$BIN" >&2
  exit 1
fi

# Smoke test: version, read a file, render a page with text using only the embedded fonts.
work="$(mktemp -d)"
"$BIN" --version
"$BIN" info "$ROOT/examples/ridgeline-studio-dashboard.figcraft" >/dev/null
"$BIN" --no-system-fonts render "$ROOT/examples/ridgeline-studio-dashboard.figcraft" -o "$work/page.png"
[[ "$(head -c 8 "$work/page.png" | od -An -tx1 | tr -d ' \n')" == 89504e470d0a1a0a ]] \
  || { echo "error: render did not write a PNG" >&2; exit 1; }
echo "render: $(wc -c <"$work/page.png" | tr -d ' ') B png"

# Package: figcraft-cli/ with the binary and its licences (FigCraft's, and the embedded fonts' OFL).
pkg="$work/figcraft-cli"
mkdir -p "$pkg" "$OUT"
cp "$BIN" "$pkg/"
for f in LICENSE-MIT LICENSE-APACHE NOTICE ATTRIBUTION.md; do cp "$ROOT/$f" "$pkg/"; done
for lic in "$FONTS"/fonts/*/OFL.txt; do
  cp "$lic" "$pkg/OFL-$(basename "$(dirname "$lic")").txt"
done
tarball="$OUT/figcraft-cli-linux-x86_64.tar.gz"
tar -C "$work" --sort=name --mtime="@$SOURCE_DATE_EPOCH" --owner=0 --group=0 --numeric-owner -cf - figcraft-cli | gzip -9 -n >"$tarball"
rm -rf "$work"
sha="$(sha256sum "$tarball" | cut -d' ' -f1)"
echo "$(basename "$tarball"): $(wc -c <"$tarball" | tr -d ' ') B, sha256 $sha; figcraft-cli $(wc -c <"$BIN" | tr -d ' ') B"
