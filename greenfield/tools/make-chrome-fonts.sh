#!/usr/bin/env bash
# Regenerates assets/fonts/*.ttf (Story 2.1) from the Fontsource npm packages, which ship
# the OFL-1.1 IBM Plex and Cormorant Garamond faces as Latin-subset WOFF2. Each becomes a plain
# TTF, so the editor embeds ~50 KB per face instead of a full ~200 KB family file. The committed
# output is what the build uses; this script is the record of where it came from.
# Needs: node/npm and python3 (a venv with fonttools and brotli is created in the work dir).
# Usage: greenfield/tools/make-chrome-fonts.sh <workdir>
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
WORK="${1:?usage: make-chrome-fonts.sh <workdir>}"
OUT="$ROOT/assets/fonts"
mkdir -p "$WORK" "$OUT"
cd "$WORK"
for p in ibm-plex-sans ibm-plex-mono cormorant-garamond; do
  npm pack "@fontsource/$p@5.3.0" --silent >/dev/null
  mkdir -p "$p"
  tar xzf "fontsource-$p-5.3.0.tgz" -C "$p"
done
python3 -m venv venv
venv/bin/pip -q install fonttools==4.68.0 brotli
cat >convert.py <<'PY'
import sys
from fontTools.ttLib import TTFont
from fontTools import subset

src, dst = sys.argv[1], sys.argv[2]
text = sys.argv[3] if len(sys.argv) > 3 else None
f = TTFont(src)
if text:
    opts = subset.Options()
    opts.layout_features = ["kern"]
    opts.name_IDs = ["*"]
    opts.notdef_outline = True
    s = subset.Subsetter(opts)
    s.populate(text=text)
    s.subset(f)
f.flavor = None
f.save(dst)
PY
conv() { venv/bin/python convert.py "$@"; }
for w in 400 500 600; do
  conv "ibm-plex-sans/package/files/ibm-plex-sans-latin-$w-normal.woff2" "$OUT/IBMPlexSans-$w.ttf"
done
for w in 400 500; do
  conv "ibm-plex-mono/package/files/ibm-plex-mono-latin-$w-normal.woff2" "$OUT/IBMPlexMono-$w.ttf"
done
# The wordmark is the one word "Greenfield": subset to those letters.
conv "cormorant-garamond/package/files/cormorant-garamond-latin-400-normal.woff2" "$OUT/CormorantGaramond-400-wordmark.ttf" "Greenfield"
cp ibm-plex-sans/package/LICENSE "$OUT/OFL-IBM-Plex.txt"
cp cormorant-garamond/package/LICENSE "$OUT/OFL-Cormorant-Garamond.txt"
ls -l "$OUT"
