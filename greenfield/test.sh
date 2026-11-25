#!/usr/bin/env bash
# Rust tests for the crates our commits touch (Stories 2.1 to 2.12), one cargo invocation at a
# time: a fat-LTO link next to `cargo test` has been SIGKILLed in a 7.6 GiB VM. The filters name
# the tests our commits add or change; upstream's wider suites are upstream's to run.
# Tests that fail on upstream v0.18.1 itself are listed in test-skips.txt and skipped by name
# (cargo's --skip), never silently. Runs natively; verify/docker-test.sh runs it in the build image.
# Usage: greenfield/test.sh
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$HERE/.."
skips=()
while IFS= read -r line; do
  [[ -z "$line" || "$line" == \#* ]] && continue
  skips+=(--skip "$line")
done <"$HERE/test-skips.txt"

t() { # <cargo test args before --> <filter>
  local filter="${*: -1}"
  local args=("${@:1:$#-1}")
  echo "== cargo test ${args[*]} $filter"
  cargo test -q "${args[@]}" "$filter" -- ${skips[@]+"${skips[@]}"}
}

t -p figcraft-engine --lib inspect_revision_tests         # 2.10: revision and undo depth
t -p figcraft-engine --lib fonts::tests                   # 2.10: missing-font warning
t -p figcraft-ui-egui --lib theme                         # 2.1: tokens, contrast, startup theme
t -p figcraft-ui-egui --lib i18n                          # 2.1, 2.2: catalogs match the code
t -p figcraft-ui-egui --lib menu_contrast_tests           # 2.1
t -p figcraft-ui-egui --lib canvas_mode_tests             # 2.11
t -p figcraft-web --bin figcraft-web query_tests          # 2.11
t -p figcraft-engine --lib gesture_command_tests          # 2.12
t -p figcraft-engine --lib agent_presence_tests           # 2.4
t -p figcraft-ui-egui --lib live::agent                   # 2.4
t -p figcraft-ui-egui --lib host_sync_tests               # 2.3
echo "all passed"
