# DeepSeek-TUI TUI Shell Job Center Parity

Date: 2026-05-13

## Gap

DeepSeek-TUI exposes a `/jobs` command surface for background shell work:
`list`, `show <id>`, `poll <id>`, `wait <id>`, `stdin <id> <input>`,
`close-stdin <id>`, and `cancel <id>`.

DeepSeekCode already had local TUI background shell start, poll, wait, and
cancel commands, but the TUI command palette did not provide the full job-center
surface for listing jobs, showing accumulated output, sending stdin, or closing
stdin.

## Scope

- Add TUI actions for shell job list, show, stdin, and close-stdin.
- Add `shell` and `jobs` command aliases:
  - `shell list`, `jobs list`
  - `shell show <id>`, `jobs show <id>`
  - `shell stdin <id> <input>`, `jobs stdin <id> <input>`
  - `shell close-stdin <id>`, `jobs close-stdin <id>`
- Wire local file-backed TUI actions to the existing in-process `exec_shell`
  background job manager.
- Expose `exec_shell_list` and `exec_shell_show` as model tools alongside
  `exec_shell_wait`, `exec_shell_interact`, and `exec_shell_cancel`.
- Keep HTTP runtime TUI behavior explicit: shell job commands remain local-only.
- Document the complete shell job command surface in `docs/tui.md` and the
  DeepSeek-TUI parity plan.

## Acceptance

- `jobs list` renders known background shell jobs and their commands.
- `jobs show <id>` renders a full shell job snapshot with accumulated stdout and
  stderr sections.
- `jobs stdin <id> <input>` sends input to a running background job.
- `jobs close-stdin <id>` closes stdin and can unblock stdin-reading jobs.
- Existing `shell poll|wait|cancel` and `jobs poll|wait|cancel` behavior remains
  unchanged.
- The model registry includes `exec_shell_list` and `exec_shell_show`.
- Focused and full Rust test gates pass.

## Verification

- `cargo test shell --lib`: 38 passed.
- `cargo test tui --lib`: 102 passed.
- `cargo fmt --check`: passed.
- `git diff --check`: passed.
- `cargo test`: 1069 passed.
- `cargo package --allow-dirty`: passed.
