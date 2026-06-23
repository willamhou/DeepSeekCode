#!/usr/bin/env bash
# Set up the calc bugfix fixture and launch `deepseek tui` under asciinema for
# a human-driven full-screen recording. The TUI is interactive (approval modal,
# /diff, rollback) and cannot be driven non-interactively, so this script only
# prepares the environment and starts the recording — the operator types the
# task prompt and quits the TUI when done. asciinema stops on TUI exit; the GIF
# is rendered with agg if available.
#
# Requires: asciinema, deepseek binary, DEEPSEEK_API_KEY exported, and a real
# TTY. agg is optional (only needed for the GIF render).
#
# Usage:
#   docs/demo/record-calc-bugfix-tui.sh [--no-gif] [--keep-fixture]
#
# Env overrides:
#   DEEPSEEK_BIN     path to the deepseek binary (default: target/debug/deepseek)
set -euo pipefail

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
cast_out="$repo_root/docs/demo/deepseek-code-calc-bugfix-tui.cast"
gif_out="$repo_root/docs/demo/deepseek-code-calc-bugfix-tui.gif"
render_gif=1
keep_fixture=0

while (($#)); do
  case "$1" in
    --no-gif) render_gif=0 ;;
    --keep-fixture) keep_fixture=1 ;;
    -h|--help) sed -n '1,/^set -euo pipefail$/p' "$0" | sed 's/^# \{0,1\}//' ; exit 0 ;;
    *) echo "unknown arg: $1" >&2 ; exit 2 ;;
  esac
  shift
done

fail() { echo "error: $*" >&2 ; exit 1 ; }

deepseek_bin="${DEEPSEEK_BIN:-$repo_root/target/debug/deepseek}"
[[ -x "$deepseek_bin" ]] || fail "deepseek binary not executable: $deepseek_bin (build it with cargo build --bin deepseek, or set DEEPSEEK_BIN)"
[[ -n "${DEEPSEEK_API_KEY:-}" ]] || fail "DEEPSEEK_API_KEY is required for a model-backed recording"
command -v asciinema >/dev/null 2>&1 || fail "asciinema not installed (pip install --user asciinema, or uv tool install asciinema)"
[[ -t 0 && -t 1 ]] || fail "stdin/stdout must be a real terminal (this script drives an interactive TUI)"
if (( render_gif )) && ! command -v agg >/dev/null 2>&1 ; then
  echo "warning: agg not installed; will produce only the .cast (cargo install --git https://github.com/asciinema/agg)" >&2
  render_gif=0
fi

fixture="${TMPDIR:-/tmp}/deepseek-calc-bugfix-demo"
rm -rf "$fixture"
mkdir -p "$fixture/src"

cat >"$fixture/Cargo.toml" <<'EOF'
[package]
name = "calc"
version = "0.1.0"
edition = "2021"

[dependencies]
EOF

cat >"$fixture/src/lib.rs" <<'EOF'
//! Tiny arithmetic helpers.

pub fn add(a: i64, b: i64) -> i64 {
    a - b
}

pub fn double(a: i64) -> i64 {
    add(a, a)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_returns_sum() {
        assert_eq!(add(2, 3), 5);
    }

    #[test]
    fn double_is_twice() {
        assert_eq!(double(21), 42);
    }
}
EOF

git -C "$fixture" init -q
git -C "$fixture" -c user.email="demo@deepseekcode.local" -c user.name="DeepSeekCode Demo" commit -q --allow-empty -m "init"
git -C "$fixture" add Cargo.toml src/lib.rs
git -C "$fixture" -c user.email="demo@deepseekcode.local" -c user.name="DeepSeekCode Demo" commit -q -m "Add calc with a failing add test"

cat <<EOF

Fixture is ready at $fixture (2 failing tests).

A TUI will open under asciinema. In Agent mode, paste this task prompt:

  Two tests in this crate fail. Run cargo test, find the bug in src/lib.rs,
  fix it, and re-run cargo test until all tests pass.

Show: failing tests -> approval modal -> apply_patch -> green tests -> /diff.
Quit the TUI (e.g. Ctrl-C / Esc) and the recording stops with it.

EOF
read -rp "Press Enter to start recording... " _

cd "$fixture"
TERM=xterm-256color asciinema rec "$cast_out" --overwrite -c "$deepseek_bin tui"

if (( render_gif )) ; then
  echo "rendering $gif_out"
  agg "$cast_out" "$gif_out"
fi

if (( keep_fixture )) ; then
  echo "fixture kept at $fixture"
else
  rm -rf "$fixture"
fi

echo "cast: $cast_out"
(( render_gif )) && echo "gif:  $gif_out"
