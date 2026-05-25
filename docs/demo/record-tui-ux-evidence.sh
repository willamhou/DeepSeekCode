#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage: docs/demo/record-tui-ux-evidence.sh [--dry-run] [--keep] [--help]

Capture deterministic TUI first-run/trust evidence without model calls:
small repo -> runtime session -> `deepseek tui --once` snapshots -> validated
provider/auth/trust/network setup guidance -> SVG render.

Environment:
  DEEPSEEK_TUI_UX_BIN       DeepSeekCode binary. Defaults to target/debug/deepseek,
                            PATH deepseek, then cargo build --bin deepseek.
  DEEPSEEK_TUI_UX_OUT       Evidence log path. Defaults to
                            docs/demo/deepseek-code-tui-ux-evidence.log.
  DEEPSEEK_TUI_UX_SVG       SVG path. Defaults to
                            docs/demo/deepseek-code-tui-ux-evidence.svg.
  DEEPSEEK_TUI_UX_WORKDIR   Parent directory for the disposable repo.
USAGE
}

repo_root=$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
out=${DEEPSEEK_TUI_UX_OUT:-"$repo_root/docs/demo/deepseek-code-tui-ux-evidence.log"}
svg_out=${DEEPSEEK_TUI_UX_SVG:-"$repo_root/docs/demo/deepseek-code-tui-ux-evidence.svg"}
work_parent=${DEEPSEEK_TUI_UX_WORKDIR:-"${TMPDIR:-/tmp}"}
dry_run=0
keep=0

for arg in "$@"; do
  case "$arg" in
    --dry-run)
      dry_run=1
      ;;
    --keep)
      keep=1
      ;;
    --help|-h)
      usage
      exit 0
      ;;
    *)
      echo "unknown argument: $arg" >&2
      usage >&2
      exit 2
      ;;
  esac
done

if [[ "$dry_run" -eq 1 ]]; then
  cat <<EOF
DeepSeekCode TUI UX evidence dry run
repo root: $repo_root
log: $out
svg: $svg_out
plan:
- create a disposable small repo with a durable runtime session
- run deepseek tui --once with no project config and expect provider guidance
- add provider/model config and expect API key guidance
- add .env API key marker and expect trust guidance
- add workspace trust marker and expect network policy guidance
- render docs/demo/deepseek-code-tui-ux-evidence.svg from the validated log
EOF
  exit 0
fi

if [[ -n "${DEEPSEEK_TUI_UX_BIN:-}" ]]; then
  deepseek_bin=$DEEPSEEK_TUI_UX_BIN
elif [[ -x "$repo_root/target/debug/deepseek" ]]; then
  deepseek_bin="$repo_root/target/debug/deepseek"
elif command -v deepseek >/dev/null 2>&1; then
  deepseek_bin=$(command -v deepseek)
else
  (cd "$repo_root" && cargo build --bin deepseek)
  deepseek_bin="$repo_root/target/debug/deepseek"
fi

run_id=$(date -u +"%Y%m%d-%H%M%S")
demo_repo="$work_parent/deepseek-code-tui-ux-evidence-$run_id"
demo_home="$work_parent/deepseek-code-tui-ux-home-$run_id"
trust_file="$work_parent/deepseek-code-tui-ux-trust-$run_id.json"
raw_dir="$work_parent/deepseek-code-tui-ux-raw-$run_id"
demo_key_env=DSC_TUI_KEY

cleanup() {
  if [[ "$keep" -eq 0 ]]; then
    rm -rf "$demo_repo" "$demo_home" "$trust_file" "$raw_dir"
  fi
}
trap cleanup EXIT

mkdir -p "$demo_repo/.dscode/runtime/sessions" "$demo_repo/.dscode/runtime/threads" "$demo_home" "$raw_dir"
cat > "$demo_repo/README.md" <<'EOF'
# Tiny TUI UX Evidence Repo

This disposable repo exists only to capture TUI first-run guidance.
EOF

cat > "$demo_repo/.dscode/runtime/sessions/session-tui-ux.json" <<EOF
{"id":"session-tui-ux","created_at":"epoch+1","updated_at":"epoch+2","title":"TUI UX dogfood","workspace":"$demo_repo","status":"active","active_thread_id":"thread-tui-ux","thread_count":1,"session_budget_microusd":null}
EOF

cat > "$demo_repo/.dscode/runtime/threads/thread-tui-ux.json" <<EOF
{"id":"thread-tui-ux","session_id":"session-tui-ux","created_at":"epoch+1","updated_at":"epoch+2","title":"First-run guidance","workspace":"$demo_repo","model":"deepseek-v4-pro","mode":"agent","status":"active","latest_turn_id":null,"event_seq":0,"session_budget_microusd":null}
EOF

capture_tui() {
  local name=$1
  local raw="$raw_dir/$name.raw"
  (
    cd "$demo_repo"
    HOME="$demo_home" \
      DSCODE_WORKSPACE_TRUST_FILE="$trust_file" \
      env -u DSCODE_PROFILE \
        -u DEEPSEEK_PROFILE \
        -u DEEPSEEK_BASE_URL \
        -u DEEPSEEK_MODEL \
        -u DEEPSEEK_API_KEY_ENV \
        -u DSCODE_NETWORK_DEFAULT \
        -u DEEPSEEK_NETWORK_DEFAULT \
        -u DSCODE_NETWORK_ALLOW \
        -u DEEPSEEK_NETWORK_ALLOW \
        -u DSCODE_NETWORK_DENY \
        -u DEEPSEEK_NETWORK_DENY \
        -u DSCODE_NETWORK_AUDIT \
        -u DEEPSEEK_NETWORK_AUDIT \
        "$deepseek_bin" tui --once
  ) > "$raw"
}

require_raw() {
  local name=$1
  local needle=$2
  if ! grep -Fq "$needle" "$raw_dir/$name.raw"; then
    echo "missing expected TUI output in $name: $needle" >&2
    echo "--- $name raw ---" >&2
    cat "$raw_dir/$name.raw" >&2
    exit 1
  fi
}

observed_line() {
  local name=$1
  local first=$2
  local second=$3
  local third=$4
  printf 'observed: %s | %s | %s\n' "$first" "$second" "$third"
  require_raw "$name" "$first"
  require_raw "$name" "$second"
  require_raw "$name" "$third"
}

capture_tui "01-provider"
require_raw "01-provider" "Setup guide"
require_raw "01-provider" "Next setup: Choose provider"
require_raw "01-provider" "Jump: /setup provider"

mkdir -p "$demo_repo/.dscode"
cat > "$demo_repo/.dscode/config.toml" <<EOF
model.base_url = "https://api.deepseek.com"
model.api_key_env = "$demo_key_env"
model.model = "deepseek-v4-pro"
EOF

capture_tui "02-auth"
require_raw "02-auth" "Next setup: Store API key"
require_raw "02-auth" "Jump: /setup auth $demo_key_env"

cat > "$demo_repo/.env" <<EOF
$demo_key_env=demo-key-not-secret
EOF

capture_tui "03-trust"
require_raw "03-trust" "Next setup: Inspect trust"
require_raw "03-trust" "Jump: /setup trust"

cat > "$trust_file" <<EOF
{"workspaces":{},"trust_modes":{"$demo_repo":true}}
EOF

capture_tui "04-network"
require_raw "04-network" "Next setup: Review network"
require_raw "04-network" "Jump: /setup network"

mkdir -p "$(dirname -- "$out")"
{
  echo "# DeepSeekCode TUI UX evidence"
  echo "workspace: /tmp/deepseek-code-tui-ux-evidence"
  echo "binary: ${deepseek_bin/#$repo_root\//}"
  echo
  echo "$ deepseek tui --once # fresh repo"
  echo "case: fresh repo without provider/model/auth"
  echo "expect: Setup guide -> Next setup: Choose provider -> Jump: /setup provider"
  observed_line "01-provider" "Setup guide" "Next setup: Choose provider" "Jump: /setup provider"
  echo
  echo "$ deepseek tui --once # provider/model configured"
  echo "case: provider/model configured without API key"
  echo "expect: Next setup: Store API key -> Jump: /setup auth $demo_key_env"
  observed_line "02-auth" "Next setup: Store API key" "Jump: /setup auth $demo_key_env" "Status: ready"
  echo
  echo "$ deepseek tui --once # API key available from .env"
  echo "case: API key present, trust not reviewed"
  echo "expect: Next setup: Inspect trust -> Jump: /setup trust"
  observed_line "03-trust" "Next setup: Inspect trust" "Jump: /setup trust" "Status: ready"
  echo
  echo "$ deepseek tui --once # trust reviewed"
  echo "case: trust reviewed, network policy missing"
  echo "expect: Next setup: Review network -> Jump: /setup network"
  observed_line "04-network" "Next setup: Review network" "Jump: /setup network" "Status: ready"
  echo
  echo "status: ok"
} > "$out"

node "$repo_root/docs/demo/render-tui-ux-evidence-svg.js" "$out" --out "$svg_out"

echo "wrote $out"
echo "wrote $svg_out"
if [[ "$keep" -eq 1 ]]; then
  echo "kept $demo_repo"
fi
