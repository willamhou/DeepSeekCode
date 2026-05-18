#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: docs/demo/record-readme-demo.sh [--help]

Regenerates the README TUI demo assets from the deterministic TUI snapshot:
  - docs/demo/deepseek-code-tui-demo.svg animated SVG
  - docs/demo/deepseek-code-tui.svg static SVG fallback

Environment:
  DEEPSEEK_DEMO_BIN   DeepSeekCode binary to use. Defaults to target/debug/deepseek,
                      then PATH deepseek, then builds target/debug/deepseek.
  SVG_TERM_BIN        svg-term executable. Defaults to svg-term on PATH.
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --help|-h)
      usage
      exit 0
      ;;
    *)
      echo "unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

script_dir=$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
repo_root=$(CDPATH= cd -- "$script_dir/../.." && pwd)
svg_term=${SVG_TERM_BIN:-svg-term}
animated_out="$repo_root/docs/demo/deepseek-code-tui-demo.svg"
static_out="$repo_root/docs/demo/deepseek-code-tui.svg"

if [[ -n "${DEEPSEEK_DEMO_BIN:-}" ]]; then
  deepseek_bin=$DEEPSEEK_DEMO_BIN
elif [[ -x "$repo_root/target/debug/deepseek" ]]; then
  deepseek_bin="$repo_root/target/debug/deepseek"
elif command -v deepseek >/dev/null 2>&1; then
  deepseek_bin=$(command -v deepseek)
else
  echo "target/debug/deepseek not found; building debug binary" >&2
  cargo build --manifest-path "$repo_root/Cargo.toml" --bin deepseek
  deepseek_bin="$repo_root/target/debug/deepseek"
fi

if [[ ! -x "$deepseek_bin" ]]; then
  echo "DeepSeekCode binary is not executable: $deepseek_bin" >&2
  exit 1
fi

if ! command -v "$svg_term" >/dev/null 2>&1; then
  echo "svg-term is required to regenerate README demo assets." >&2
  echo "Install svg-term or set SVG_TERM_BIN to an executable path." >&2
  exit 1
fi

animated_command='bash -lc '"'"'"$DEEPSEEK_DEMO_BIN" tui --demo --once | sed -e "s/^\\\"//" -e "s/\\\"$//" | while IFS= read -r line; do printf "%s\\n" "$line"; sleep 0.08; done; sleep 1.5'"'"''
static_command='bash -lc '"'"'"$DEEPSEEK_DEMO_BIN" tui --demo --once | sed -e "s/^\\\"//" -e "s/\\\"$//"; sleep 1'"'"''

DEEPSEEK_DEMO_BIN="$deepseek_bin" "$svg_term" \
  --command "$animated_command" \
  --out "$animated_out" \
  --width 122 \
  --height 36 \
  --window \
  --no-cursor

DEEPSEEK_DEMO_BIN="$deepseek_bin" "$svg_term" \
  --command "$static_command" \
  --out "$static_out" \
  --width 122 \
  --height 36 \
  --window \
  --no-cursor \
  --at 1000

grep -q "@keyframes" "$animated_out" || {
  echo "animated README demo is missing SVG keyframes: $animated_out" >&2
  exit 1
}
grep -q "animation-duration" "$animated_out" || {
  echo "animated README demo is missing animation duration: $animated_out" >&2
  exit 1
}
grep -q "DeepSeekCode" "$animated_out" || {
  echo "animated README demo does not render DeepSeekCode: $animated_out" >&2
  exit 1
}
grep -q "DeepSeekCode" "$static_out" || {
  echo "static README demo does not render DeepSeekCode: $static_out" >&2
  exit 1
}
if grep -q "@keyframes" "$static_out"; then
  echo "static README snapshot unexpectedly contains animation keyframes: $static_out" >&2
  exit 1
fi

echo "updated: $animated_out"
echo "updated: $static_out"
