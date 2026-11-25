#!/usr/bin/env bash
# Install the pinned web-build toolchain inside the Debian build image (versions.env): binaryen
# from Debian's snapshot archive (sha256-checked), brotli, the wasm32 target, trunk and
# wasm-bindgen-cli. Skips trunk/wasm-bindgen when the cached binaries already match.
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091 # versions.env: plain KEY=value pins
. "$HERE/versions.env"

arch="$(dpkg --print-architecture)"
want_var="BINARYEN_DEB_SHA256_$arch"
want="${!want_var:-}"
[[ -n "$want" ]] || { echo "error: no pinned binaryen .deb for $arch" >&2; exit 1; }
deb="$(mktemp -d)/binaryen.deb"
curl -fsSL --retry 3 -o "$deb" "${BINARYEN_DEB_URL//ARCH/$arch}"
echo "$want  $deb" | sha256sum -c --quiet
apt-get update -qq
apt-get install -y -qq --no-install-recommends brotli "$deb" >/dev/null

rustup target add wasm32-unknown-unknown
if ! trunk --version 2>/dev/null | grep -qx "trunk $TRUNK_VERSION"; then
  cargo install trunk --version "$TRUNK_VERSION" --locked --quiet
fi
if ! wasm-bindgen --version 2>/dev/null | grep -qx "wasm-bindgen $WASM_BINDGEN_VERSION"; then
  cargo install wasm-bindgen-cli --version "$WASM_BINDGEN_VERSION" --locked --quiet
fi
rustc --version
trunk --version
wasm-bindgen --version
wasm-opt --version
