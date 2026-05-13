# DeepSeek-TUI MCP Long-Tail Closure Audit

Date: 2026-05-13

Status: completed

## Gap

Phase G2 still carried a broad "audit the remaining DeepSeek-TUI MCP long
tail" item after shell-session and RLM exposure. Follow-up slices have now
covered the main compatibility helpers that were agent-visible but missing from
MCP/ACP.

## Audit Result

MCP/ACP now expose the DeepSeek-TUI compatibility helper clusters through
explicit safety contracts:

- Read/default: workspace read/search, `web_run`, Git/GitHub context,
  diagnostics, review, recall, tool search, `load_skill`,
  `request_user_input`, `notify`, `image_ocr`, and inline `pandoc_convert`.
- Durable write approvals: file writes/edits/moves/deletes, patch/revert,
  GitHub writes, runtime task/automation/agent mutations, `pandoc_convert`
  with `output_path`, `note`, and enabled `remember`.
- Trusted side effects or durable approvals: shell sessions, `run_shell`,
  `run_tests`, stateful RLM Python, model-running RLM tools, and
  `image_analyze`.

Remaining agent-visible names that are not exported as same-name MCP server
tools are not currently DeepSeek-TUI server-surface blockers:

- `task_*`, `automation_*`, and `agent_*` are represented by the structured
  `runtime_*` MCP tools with durable approvals.
- `todo_*`, `checklist_*`, and `update_plan` are session-local planning state,
  not stable workspace/runtime server operations.
- `mcp_*` bridge tools are MCP-client operations for calling other configured
  MCP servers, not useful as self-recursive `serve --mcp` tools.
- `pr_attempt_*` and `task_gate_run` are product-specific workflow helpers that
  can be revisited after the DeepSeek-TUI parity pass.

## Remaining

The original Phase G2 compatibility remainder was ACP transport behavior: true
process-level stdout/stderr streaming while tools are still executing, beyond
bounded post-execution output progress chunks. A later ACP live shell streaming
slice landed opt-in `stream=true` / `follow=true` updates for `exec_shell` and
`task_shell_start`, so this audit no longer carries an open MCP/ACP server
surface blocker.

## Verification

- `/home/willamhou/.cargo/bin/cargo test mcp_ --lib`
- `/home/willamhou/.cargo/bin/cargo test acp_ --lib`
- `/home/willamhou/.cargo/bin/cargo fmt --check`
- `git diff --check`
