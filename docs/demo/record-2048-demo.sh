#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: docs/demo/record-2048-demo.sh [--dry-run] [--serve] [--cleanup] [--api-key-stdin] [--redaction-self-test] [--help]

Records a model-backed DeepSeekCode launch demo against an empty disposable
web repo:
empty repo -> deepseek exec builds a playable 2048 game -> validate files ->
git diff --stat -> optional local preview server.

Environment:
  DEEPSEEK_API_KEY             Required for model-backed demo evidence.
  DEEPSEEK_2048_KEY_FILE       Read DEEPSEEK_API_KEY from a first-line key file
                               outside this repository when DEEPSEEK_API_KEY is unset.
  DEEPSEEK_2048_BIN            DeepSeekCode binary to run. Defaults to
                               target/debug/deepseek, then PATH deepseek, then
                               builds target/debug/deepseek.
  DEEPSEEK_2048_BUDGET         Agent step budget. Defaults to 16.
  DEEPSEEK_2048_OUT            Transcript path. Defaults to a timestamped file
                               in docs/demo/.
  DEEPSEEK_2048_WORKDIR        Parent directory for the disposable repo.
  DEEPSEEK_2048_PROMPT         Override the coding task prompt.

The transcript is source evidence for GIF/MP4 capture. Review generated media
before committing it. Do not publish a run unless it used a real model call.
EOF
}

dry_run=0
serve=0
cleanup=0
api_key_stdin=0
redaction_self_test=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --dry-run)
      dry_run=1
      shift
      ;;
    --serve)
      serve=1
      shift
      ;;
    --cleanup)
      cleanup=1
      shift
      ;;
    --api-key-stdin)
      api_key_stdin=1
      shift
      ;;
    --redaction-self-test)
      redaction_self_test=1
      shift
      ;;
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
run_id=$(date +%Y%m%d-%H%M%S)
demo_budget=${DEEPSEEK_2048_BUDGET:-16}
demo_out=${DEEPSEEK_2048_OUT:-"$repo_root/docs/demo/deepseek-code-2048-demo-$run_id.log"}
work_parent=${DEEPSEEK_2048_WORKDIR:-"${TMPDIR:-/tmp}"}
demo_repo="$work_parent/deepseek-code-2048-demo-$run_id"
demo_prompt=${DEEPSEEK_2048_PROMPT:-"Build a playable 2048 web game in this empty repository using plain HTML, CSS, and JavaScript. Create index.html, styles.css, and app.js. Requirements: a 4x4 board, keyboard arrow controls, tile merging, score tracking, random new tiles, win/game-over messaging, and a restart button. Keep the UI polished, lightweight, and concise. Use small, clear patches for each file. After writing files, run a validation command that verifies index.html, styles.css, and app.js exist and prints their byte sizes."}

redact_demo_stream() {
  awk '
    function redact_all(line, needle, replacement,    out, pos) {
      if (length(needle) == 0) {
        return line
      }
      out = ""
      while ((pos = index(line, needle)) > 0) {
        out = out substr(line, 1, pos - 1) replacement
        line = substr(line, pos + length(needle))
      }
      return out line
    }
    BEGIN {
      names[1] = "DEEPSEEK_API_KEY"
      names[2] = "OPENAI_API_KEY"
      names[3] = "ANTHROPIC_API_KEY"
      names[4] = "DEEPSEEK_2048_KEY_FILE"
      for (i = 1; i <= 4; i++) {
        value = ENVIRON[names[i]]
        if (length(value) > 0) {
          count += 1
          secrets[count] = value
          replacements[count] = "<" names[i] ":redacted>"
        }
      }
    }
    {
      line = $0
      for (i = 1; i <= count; i++) {
        line = redact_all(line, secrets[i], replacements[i])
      }
      print line
    }
  '
}

if [[ "$redaction_self_test" -eq 1 ]]; then
  previous_key_set=0
  previous_key_value=
  if [[ -n "${DEEPSEEK_API_KEY+x}" ]]; then
    previous_key_set=1
    previous_key_value=$DEEPSEEK_API_KEY
  fi
  export DEEPSEEK_API_KEY="${DEEPSEEK_API_KEY:-demo-secret-for-redaction}"
  test_secret=$DEEPSEEK_API_KEY
  output=$(printf 'before %s after\n' "$test_secret" | redact_demo_stream)
  if [[ "$previous_key_set" -eq 1 ]]; then
    DEEPSEEK_API_KEY=$previous_key_value
    export DEEPSEEK_API_KEY
  else
    unset DEEPSEEK_API_KEY
  fi
  if [[ "$output" == *"$test_secret"* ]]; then
    echo "redaction self-test failed: secret remained in output" >&2
    exit 1
  fi
  if [[ "$output" != *"<DEEPSEEK_API_KEY:redacted>"* ]]; then
    echo "redaction self-test failed: redaction marker missing" >&2
    exit 1
  fi
  echo "redaction self-test ok"
  exit 0
fi

if [[ "$dry_run" -eq 1 ]]; then
  echo "DeepSeekCode 2048 demo dry run"
  echo "repo_root: $repo_root"
  echo "demo_repo: $demo_repo"
  echo "transcript: $demo_out"
  echo "budget: $demo_budget"
  echo "serve: $serve"
  echo "prompt: $demo_prompt"
  echo "status: dry-run only; no API call, repository creation, or transcript write"
  exit 0
fi

if [[ -z "${DEEPSEEK_API_KEY:-}" && -n "${DEEPSEEK_2048_KEY_FILE:-}" ]]; then
  if [[ ! -f "$DEEPSEEK_2048_KEY_FILE" ]]; then
    echo "DEEPSEEK_2048_KEY_FILE does not exist: $DEEPSEEK_2048_KEY_FILE" >&2
    exit 1
  fi
  key_file_dir=$(CDPATH= cd -- "$(dirname -- "$DEEPSEEK_2048_KEY_FILE")" && pwd)
  key_file_abs="$key_file_dir/$(basename -- "$DEEPSEEK_2048_KEY_FILE")"
  case "$key_file_abs" in
    "$repo_root"/*)
      echo "DEEPSEEK_2048_KEY_FILE must live outside this repository: $key_file_abs" >&2
      exit 1
      ;;
  esac
  IFS= read -r DEEPSEEK_API_KEY < "$key_file_abs"
  export DEEPSEEK_API_KEY
fi

if [[ -z "${DEEPSEEK_API_KEY:-}" && "$api_key_stdin" -eq 1 ]]; then
  IFS= read -r DEEPSEEK_API_KEY
  export DEEPSEEK_API_KEY
fi

if [[ -z "${DEEPSEEK_API_KEY:-}" ]]; then
  echo "DEEPSEEK_API_KEY is required for model-backed 2048 demo evidence." >&2
  echo "Use DEEPSEEK_2048_KEY_FILE outside the repo or --api-key-stdin to avoid putting the key in shell history." >&2
  exit 1
fi

if [[ -n "${DEEPSEEK_2048_BIN:-}" ]]; then
  deepseek_bin=$DEEPSEEK_2048_BIN
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

mkdir -p "$demo_repo"
mkdir -p "$(dirname -- "$demo_out")"

git -C "$demo_repo" init -q
git -C "$demo_repo" config user.email "demo@deepseekcode.local"
git -C "$demo_repo" config user.name "DeepSeekCode Demo"
cat > "$demo_repo/README.md" <<'EOF'
# DeepSeekCode 2048 Demo

This disposable repository starts empty except for this README. The demo asks
DeepSeekCode to build a playable 2048 web game with plain HTML, CSS, and
JavaScript.
EOF
git -C "$demo_repo" add README.md
git -C "$demo_repo" commit -q -m "Create empty 2048 demo repo"

run_session() {
  cd "$demo_repo"
  echo "DeepSeekCode 2048 model-backed demo"
  echo "workspace: $demo_repo"
  echo
  echo "$ find . -maxdepth 2 -type f | sort"
  find . -maxdepth 2 -type f | sort
  echo
  echo "$ DSCODE_AUTO_APPROVE_WRITES=1 DSCODE_AUTO_APPROVE_SHELL=1 $deepseek_bin exec --budget $demo_budget \"<2048 prompt>\""
  local exec_status=0
  DSCODE_AUTO_APPROVE_WRITES=1 \
    DSCODE_AUTO_APPROVE_SHELL=1 \
    "$deepseek_bin" exec --budget "$demo_budget" "$demo_prompt" || exec_status=$?
  if [[ "$exec_status" -ne 0 ]]; then
    echo "deepseek exec failed with status $exec_status" >&2
    return "$exec_status"
  fi
  echo
  echo "$ test -s index.html && test -s styles.css && test -s app.js"
  local missing=0
  for required_file in index.html styles.css app.js; do
    if [[ ! -s "$required_file" ]]; then
      echo "missing or empty required file: $required_file" >&2
      missing=1
    fi
  done
  if [[ "$missing" -ne 0 ]]; then
    return 1
  fi
  echo "required files present"
  echo
  echo "$ wc -c index.html styles.css app.js"
  wc -c index.html styles.css app.js
  echo
  echo "$ git diff --stat"
  git diff --stat
  echo
  echo "$ git diff -- index.html styles.css app.js | sed -n '1,220p'"
  git diff -- index.html styles.css app.js | sed -n '1,220p'
  if [[ "$serve" -eq 1 ]]; then
    echo
    echo "$ python3 -m http.server 4173"
    echo "Preview URL: http://127.0.0.1:4173"
    echo "Stop the server with Ctrl+C after recording browser gameplay."
    python3 -m http.server 4173
  else
    echo
    echo "Preview command: cd $demo_repo && python3 -m http.server 4173"
    echo "Preview URL: http://127.0.0.1:4173"
  fi
}

set +e
run_session 2>&1 | redact_demo_stream | tee "$demo_out"
session_status=${PIPESTATUS[0]}
set -e

echo
echo "transcript: $demo_out"
echo "demo repo: $demo_repo"
if [[ "$session_status" -eq 0 ]]; then
  echo "status: ok"
else
  echo "status: failed ($session_status)"
fi

if [[ "$cleanup" -eq 1 ]]; then
  rm -rf "$demo_repo"
  echo "demo repo removed"
fi

exit "$session_status"
