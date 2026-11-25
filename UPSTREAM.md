# Greenfield editor: fork notes and upstream sync

This is Greenfield's fork of [FigCraft](https://github.com/storytold/figcraft) (the web editor
embedded in the Greenfield app). `main` is upstream's history at the release in
`greenfield/versions.env` (`UPSTREAM_REF`), plus our commits on top. `git log upstream-tag..main`
reads like the changes we carry; each says its Story. Upstream's `LICENSE-*`, `NOTICE` and
`ATTRIBUTION.md` are untouched and ship in every build.

## Layout

- `greenfield/versions.env` pins the build image digest, binaryen, wasm-bindgen, trunk, craft-fonts and the 25 MiB size gate. Different tool versions give different bytes.
- `greenfield/build.sh` is the whole build: marks check on the source, `trunk build --release`, size gate, marks check on the site and its self-tests, site plus licences into `.cache/site/`. `greenfield/toolchain.sh` installs the pinned tools in the image. `greenfield/verify/docker-build.sh` runs both in a clean container; that is the only supported way to build locally.
- `greenfield/test.sh` runs the Rust tests of the crates our commits touch, one cargo invocation at a time. Known upstream failures go in `greenfield/test-skips.txt`, each with a reason.
- `greenfield/check-marks.sh`, `check-marks.allow`, `upstream-marks/`: fail the build when the FigCraft or ArtCraft name or logo is user-visible (Story 2.2).
- `greenfield/tools/`: `gen-marks.py` and `make-chrome-fonts.sh` regenerate the committed icons, mark bitmap and chrome fonts. `rebrand-strings.pl` renamed the product in strings; keep it for syncs, because a new upstream string that says FigCraft fails the marks check and this script fixes it.
- `.github/workflows/build.yml` is the only workflow. Upstream's were removed (they publish FigCraft releases and need its secrets).
- The brand SVGs next to `apps/figcraft-web/index.html` are copies of greenfield's `apps/web/public/greenfield-*.svg`. Change them in both places, then rerun `gen-marks.py`.

## Releases

Every push to `main` that passes build and tests publishes release `editor-<12-char sha>` with the
site zip and its sha256 in the notes and the job summary. Pin it in greenfield's
`packages/bridge/editor-release.json`. Pull requests build and test but never publish.

## Sync routine

Cadence (Story 2.7): once a month, and on any upstream release we want. Rebase, never merge, so
our commits stay a readable stack.

1. `git fetch upstream --tags` (`upstream` is `https://github.com/storytold/figcraft`, read only: never push there).
2. Pick the target tag `vX.Y.Z`. Create the branch `upstream` at it and the tag `upstream/vX.Y.Z`, push both to `origin`.
3. `git switch -c sync/vX.Y.Z main && git rebase --onto vX.Y.Z <previous UPSTREAM_REF> sync/vX.Y.Z`. Resolve conflicts commit by commit, keeping each commit's meaning. A modify/delete conflict on an upstream workflow file is resolved by deleting the file.
4. Run `perl greenfield/tools/rebrand-strings.pl` over any file the marks check reports (`greenfield/check-marks.sh src .`), and add new craft-fonts or icon hashes to `greenfield/upstream-marks/` if upstream changed its icons.
5. Bump `UPSTREAM_REF` and `UPSTREAM_SHA` in `greenfield/versions.env`. Check that `WASM_BINDGEN_VERSION` still equals wasm-bindgen in `Cargo.lock` and that `CRAFT_FONTS_SHA` matches the ref upstream's release CI uses.
6. `greenfield/verify/docker-test.sh` and `greenfield/verify/docker-build.sh`. The size gate and marks check must pass. Push `sync/vX.Y.Z`; CI repeats both on it (a branch push builds and tests but never publishes).
7. Move `main` to the rebased stack: `git push --force-with-lease=main:<old main sha> origin sync/vX.Y.Z:main`. A rebase rewrites our commits, so this is the one sanctioned force-push to `main`, done by the operator at a sync and at no other time. CI then builds `main` and publishes the release. (If you would rather never rewrite `main`, `git merge vX.Y.Z` into `main` instead of rebasing; the cost is one merge commit with the conflicts folded in.)
8. In greenfield: pin the new release in `packages/bridge/editor-release.json`, run `pnpm --filter @greenfield/bridge generate` (regenerates the command types; review the diff and the `COMMANDS_HASH` change), run the app's tests, open the PR.
