#!/usr/bin/env bash
# rect-diff runner: browser-rect → core dump_page --json → diff.mjs, one page.
#
# Usage: run-page.sh <page> [--tol-box=N] [--tol-text=N]
#   page ∈ home/settings/inventory/mail/shop/character/form/lab
# Artifacts: out/<page>/browser-<page>.json + core-<page>.json (gitignored)
# Exit: 0/1/2 = diff.mjs passthrough (2 also usage here); 3 = infra failure
# (browser-rect or core dump step crashed — NOT a layout regression) — 报告产出为主门，exit 1 ≠ 任务失败
set -euo pipefail

if [ $# -lt 1 ]; then
  echo "usage: run-page.sh <page> [--tol-box=N] [--tol-text=N]" >&2
  exit 2
fi
PAGE="$1"
shift
TOL_BOX=1
TOL_TEXT=3
for a in "$@"; do
  case "$a" in
    --tol-box=*) TOL_BOX="${a#--tol-box=}" ;;
    --tol-text=*) TOL_TEXT="${a#--tol-text=}" ;;
    *) echo "error: unknown arg: $a" >&2; exit 2 ;;
  esac
done

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../.." && pwd)"
OUT_DIR="$SCRIPT_DIR/out/$PAGE"
mkdir -p "$OUT_DIR"

HTML_PATH="$REPO_ROOT/showcase/showcase/$PAGE.html"
if [ ! -f "$HTML_PATH" ]; then
  echo "error: no such page: $HTML_PATH" >&2
  exit 2
fi

echo "==> 1/3 browser rect ($PAGE)"
node "$SCRIPT_DIR/browser-rect.mjs" "$HTML_PATH" "$OUT_DIR/browser-$PAGE.json" || {
  echo "error: browser-rect step failed" >&2
  exit 3
}

echo "==> 2/3 core dump ($PAGE)"
(cd "$REPO_ROOT" && cargo run -q -p loomgui_core --example dump_page -- "$PAGE" --json "$OUT_DIR/core-$PAGE.json") || {
  echo "error: core dump step failed" >&2
  exit 3
}

echo "==> 3/3 diff (tol-box=$TOL_BOX tol-text=$TOL_TEXT)"
node "$SCRIPT_DIR/diff.mjs" "$OUT_DIR/browser-$PAGE.json" "$OUT_DIR/core-$PAGE.json" --tol-box="$TOL_BOX" --tol-text="$TOL_TEXT"
