# DeepSeek-TUI Terminal Signal Restore

**Status:** implemented
**Comparison source:** `Hmbown/DeepSeek-TUI` refreshed at `/tmp/deepseek-tui-compare-20260514`; latest fetched `origin/main` `13e7957621448792beda06ec8615e33cb374adce`.

## Gap

The latest DeepSeek-TUI refresh added terminal cleanup on abnormal exits so raw
mode, mouse capture, and alternate-screen state do not leak into the user's
shell after `SIGINT`, `SIGTERM`, or panic-adjacent exits. DeepSeekCode's TUI
normal teardown restored terminal state, but it did not have a shared
best-effort emergency restore path or a signal watcher.

## Implementation

- Added a direct `signal-hook` dependency.
- Added `emergency_restore_terminal()` for best-effort raw-mode, mouse-capture,
  alternate-screen, cursor, and flush cleanup.
- Wrapped interactive TUI setup with `TerminalRestoreGuard` so panic/unwind or
  setup failures still restore the terminal.
- Installed a Unix-only signal cleanup watcher for `SIGINT`, `SIGTERM`, and
  `SIGHUP`; signal exits use the conventional `128 + signal` status.
- Kept normal TUI teardown explicit, then disarms the emergency guard.

## Verification

- `/home/willamhou/.cargo/bin/cargo fmt --check`
- `/home/willamhou/.cargo/bin/cargo test terminal_ --lib`
- `/home/willamhou/.cargo/bin/cargo check`
- `/home/willamhou/.cargo/bin/cargo test --lib -- --test-threads=1`
- `git diff --check`

## Remaining

This closes the terminal emergency-restore slice. It does not prove native
Windows ConPTY behavior or complete the broader supervisor-owned PTY polish
gap.
