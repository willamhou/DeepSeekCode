#!/usr/bin/env bash
# Record a real model-backed bugfix run through `deepseek exec` as an
# asciinema cast (and a GIF, if `agg` is installed). The fixture is created in a
# disposable temp directory: a tiny crate with a single-line subtraction bug
# whose two tests both turn green after a one-token apply_patch. The narrative
# is captured verbatim from a real PTY: failing tests → agent fix → green tests
# → git diff → green tests again.
#
# Requires: asciinema, deepseek binary, and DEEPSEEK_API_KEY exported. agg is
# optional (only needed for the GIF render).
#
# Usage:
#   docs/demo/record-calc-bugfix-exec.sh [--no-gif] [--keep-fixture]
#
# Env overrides:
#   DEEPSEEK_BIN     path to the deepseek binary (default: target/debug/deepseek)
#   DEEPSEEK_BUDGET  --budget passed to deepseek exec (default: 14)
#   DEEPSEEK_PRESET  --preset passed to deepseek exec (default: pro)
set -euo pipefail

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
cast_out="$repo_root/docs/demo/deepseek-code-calc-bugfix.cast"
gif_out="$repo_root/docs/demo/deepseek-code-calc-bugfix.gif"
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
git -C "$fixture" -c user.email="demo@deepseekcode.local" -c user.name="DeepSeekCode Demo" commit -q --allow-empty-message --allow-empty -m ""
git -C "$fixture" add Cargo.toml src/lib.rs
git -C "$fixture" -c user.email="demo@deepseekcode.local" -c user.name="DeepSeekCode Demo" commit -q -m "Add calc with a failing add test"

inner="$fixture/exec-take.sh"
cat >"$inner" <<EOF
#!/usr/bin/env bash
# Inner narrative recorded by asciinema. DEEPSEEK_API_KEY is inherited from the
# parent shell and never printed inside this script.
set -uo pipefail
export GIT_PAGER=cat PAGER=cat
cd "$fixture"
BIN="$deepseek_bin"

echo "\$ cargo test"
cargo test 2>&1 | tail -8
echo
echo '\$ deepseek exec "fix the failing tests"'
DSCODE_AUTO_APPROVE_WRITES=1 DSCODE_AUTO_APPROVE_SHELL=1 "\$BIN" exec --preset "${DEEPSEEK_PRESET:-pro}" --budget "${DEEPSEEK_BUDGET:-14}" \\
  "Two tests in this crate fail. Run cargo test, find the bug in src/lib.rs, fix it, and re-run cargo test until all tests pass. Then summarize the diff."
echo
echo "\$ git diff -- src/lib.rs"
git --no-pager diff -- src/lib.rs
echo
echo "\$ cargo test"
cargo test 2>&1 | tail -6
EOF
chmod +x "$inner"

echo "recording into $cast_out"
TERM=xterm-256color asciinema rec "$cast_out" --overwrite -c "bash $inner"

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
