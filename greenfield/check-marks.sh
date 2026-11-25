#!/usr/bin/env bash
# Story 2.2: fails when the FigCraft or ArtCraft name or logo is user-visible in the editor.
#
#   check-marks.sh src  <fork-src>     the patched source: locale catalogs, Rust string literals,
#                                      index.html, the app-icon directory
#   check-marks.sh site <site-dir>     the built site: every text file and the wasm, file names, icons
#   check-marks.sh self-test <src> <site>   negative controls: puts marks back and requires the
#                                      checks above to fail (build.sh runs this after both checks)
#
# The name is matched in its display spellings (FigCraft, Figcraft, ArtCraft, Artcraft,
# ARTCRAFT, and lowercase artcraft, which no code identifier uses). Lowercase `figcraft` is a code
# identifier (`window.figcraft`, `figcraft:` message types, crate and file names, the `.figcraft`
# extension) and is not a mark. Licence and attribution files (LICENSE-*, NOTICE, ATTRIBUTION.md,
# OFL-*.txt) name upstream on purpose; build.sh copies them into dist/ after this check, so the
# site check never sees them. Anything else that must stay goes in check-marks.allow
# (`<path regex><TAB><match regex>`; a hit is skipped when both match).
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
NAME_RE='FigCraft|Figcraft|ArtCraft|Artcraft|ARTCRAFT|artcraft'
ALLOW="$HERE/check-marks.allow"
fail=0

sha256() {
  if command -v sha256sum >/dev/null; then sha256sum "$1" | cut -d' ' -f1; else shasum -a 256 "$1" | cut -d' ' -f1; fi
}

# allowed <path> <match>: is the hit listed in check-marks.allow?
allowed() {
  [[ -f "$ALLOW" ]] || return 1
  local p m
  while IFS=$'\t' read -r p m; do
    [[ -z "$p" || "$p" == \#* ]] && continue
    [[ "$1" =~ $p && "$2" =~ $m ]] && return 0
  done <"$ALLOW"
  return 1
}

report() { # <path> <what> <match>
  allowed "$1" "$3" && return 0
  echo "mark: $1: $2: $3" >&2
  fail=1
}

# Every text-or-binary file: the name in a display spelling. Binary files (the wasm) are read as
# bytes, so a string baked into it counts; `-o` prints the match, not the line.
scan_names() { # <root> <file>...
  local root="$1" f hit
  shift
  for f in "$@"; do
    while IFS= read -r hit; do
      report "${f#"$root"/}" "name in contents" "$hit"
    done < <(LC_ALL=C grep -a -o -E ".{0,14}($NAME_RE).{0,14}" "$f" 2>/dev/null | sort -u || true)
  done
}

check_site() {
  local site="$1" f h line name hex
  [[ -f "$site/index.html" ]] || { echo "mark: $site/index.html missing" >&2; return 1; }
  grep -q '<title>Greenfield</title>' "$site/index.html" || report index.html "page title is not Greenfield" "<title>"
  local files=()
  while IFS= read -r f; do files+=("$f"); done < <(find "$site" -type f | LC_ALL=C sort)
  scan_names "$site" "${files[@]}"
  # File names that are a mark's logo or icon.
  for f in "${files[@]}"; do
    case "$(basename "$f")" in
      *figcraft*.png | *figcraft*.svg | *figcraft*.ico | *figcraft*.icns | *artcraft*) report "${f#"$site"/}" "logo or icon file name" "$(basename "$f")" ;;
    esac
  done
  # Upstream's own icons, whatever they are called.
  while read -r h name; do
    [[ -z "$h" || "$h" == \#* ]] && continue
    for f in "${files[@]}"; do
      [[ "$(sha256 "$f")" == "$h" ]] && report "${f#"$site"/}" "is upstream's icon" "$name"
    done
  done <"$HERE/upstream-marks/icons.sha256"
  # ... or a copy of them inside the wasm (include_bytes!).
  for f in "$site"/*.wasm; do
    [[ -f "$f" ]] || continue
    while read -r name hex; do
      [[ -z "$name" || "$name" == \#* ]] && continue
      if NEEDLE="$hex" perl -0777 -e '
          open my $in, "<:raw", $ARGV[0] or die; local $/; my $d = <$in>;
          my $n = pack("H*", $ENV{NEEDLE}); exit(index($d, $n) >= 0 ? 0 : 1);' "$f"; then
        report "${f#"$site"/}" "contains upstream's icon" "$name"
      fi
    done <"$HERE/upstream-marks/icon-needles.txt"
  done
}

check_src() {
  local src="$1" f ln
  # Locale catalogs: a key or value naming the product.
  while IFS= read -r f; do
    while IFS= read -r ln; do report "${f#"$src"/}" "catalog text" "${ln:0:100}"; done < <(grep -n -E "$NAME_RE" "$f" || true)
  done < <(find "$src/crates/ui-egui/locales" -name '*.toml' | LC_ALL=C sort)
  # Rust: a string literal on a non-comment line (test files hold fixtures, not shipped strings).
  while IFS= read -r f; do
    while IFS= read -r ln; do report "${f#"$src"/}" "string literal" "${ln:0:100}"; done \
      < <(grep -n -E "\"[^\"]*($NAME_RE)[^\"]*\"" "$f" | grep -v -E '^[0-9]+:[[:space:]]*//' || true)
  done < <(find "$src/crates/ui-egui/src" "$src/crates/engine/src" "$src/crates/format/src" "$src/crates/plugin/src" "$src/crates/pdf/src" \
    "$src/apps/figcraft-web/src" -name '*.rs' -not -name 'tests.rs' -not -name '*_tests.rs' | LC_ALL=C sort)
  # The page, and no upstream app-icon directory.
  scan_names "$src" "$src/apps/figcraft-web/index.html"
  [[ ! -e "$src/assets/app-icon/figcraft.svg" && ! -e "$src/assets/app-icon/figcraft-64.png" ]] \
    || report assets/app-icon "upstream app icons present" "figcraft*.png"
}

mode="${1:-}"
case "$mode" in
  src) check_src "${2:?fork source dir}" ;;
  site) check_site "${2:?site dir}" ;;
  self-test)
    SRC="${2:?fork source dir}"
    SITE="${3:?site dir}"
    tmp="$(mktemp -d)"
    trap 'rm -rf "$tmp"' EXIT
    # The real trees pass...
    "$0" src "$SRC" 2>/dev/null && "$0" site "$SITE" 2>/dev/null || { echo "self-test: the real trees fail the check" >&2; exit 1; }
    expect_fail() { # <what> <mode> <dir>
      if "$0" "$2" "$3" 2>/dev/null; then echo "self-test: $1 was not caught" >&2; exit 1; fi
      echo "self-test: $1: caught"
    }
    # ... and each mark put back is caught.
    cp -R "$SITE" "$tmp/site-title"
    sed -i.bak 's|<title>Greenfield</title>|<title>FigCraft</title>|' "$tmp/site-title/index.html" && rm "$tmp/site-title/index.html.bak"
    expect_fail "the page title put back to FigCraft" site "$tmp/site-title"
    cp -R "$SITE" "$tmp/site-text"
    printf '\n<!-- Loading ArtCraft -->\n' >>"$tmp/site-text/index.html"
    expect_fail "ArtCraft in the page" site "$tmp/site-text"
    cp -R "$SITE" "$tmp/site-icon"
    : >"$tmp/site-icon/figcraft-64.png"
    expect_fail "an upstream-named icon file" site "$tmp/site-icon"
    cp -R "$SITE" "$tmp/site-wasm"
    wasm="$(find "$tmp/site-wasm" -name '*.wasm' | head -n 1)"
    printf 'Open in FigCraft' >>"$wasm"
    expect_fail "FigCraft baked into the wasm" site "$tmp/site-wasm"
    mkdir -p "$tmp/src/crates/ui-egui/locales" "$tmp/src/crates/ui-egui/src" "$tmp/src/crates/engine/src" "$tmp/src/crates/format/src" \
      "$tmp/src/crates/plugin/src" "$tmp/src/crates/pdf/src" "$tmp/src/apps/figcraft-web/src"
    printf '"About FigCraft" = "About FigCraft"\n' >"$tmp/src/crates/ui-egui/locales/en.toml"
    expect_fail "FigCraft in a catalog" src "$tmp/src"
    : >"$tmp/src/crates/ui-egui/locales/en.toml"
    printf 'fn f() -> &str { "Open in FigCraft" }\n' >"$tmp/src/crates/ui-egui/src/x.rs"
    expect_fail "FigCraft in a string literal" src "$tmp/src"
    echo "self-test: ok"
    ;;
  *)
    echo "usage: check-marks.sh src <fork-src> | site <site-dir> | self-test <fork-src> <site-dir>" >&2
    exit 2
    ;;
esac
exit "$fail"
