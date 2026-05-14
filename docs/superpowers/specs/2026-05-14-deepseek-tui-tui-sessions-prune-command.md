# DeepSeek-TUI Parity: TUI Sessions Prune Command

## Context

The command-name parity audit passes, but a semantic audit of DeepSeek-TUI's
`crates/tui/src/commands/session.rs` found that `/sessions` also supports
`prune <days>` for housekeeping. DeepSeekCode currently supports session picker
entry points and filters, but not pruning old durable runtime sessions from the
TUI.

## Goals

- Add `sessions prune <days>` / `/sessions prune <days>` before custom slash
  fallback.
- Reject missing, zero, and non-numeric day values with DeepSeek-TUI-style
  usage errors.
- Prune local file-backed runtime sessions whose `updated_at` timestamp is
  older than the requested age.
- Remove associated session-owned runtime data so pruning does not leave linked
  thread, turn, item, event, usage, task, or automation files behind.
- Reject the command in HTTP runtime mode as local-only.
- Document the command in the TUI guide and parity plan.

## Acceptance

- `/sessions prune 30` queues a built-in TUI action instead of a custom slash
  command.
- Local action handling reports `no sessions older than 30d to prune` when
  nothing qualifies.
- Local action handling reports the number of pruned sessions when records
  qualify.
- Pruning deletes linked thread data and session-scoped task/automation records.
- Full `tui` tests continue passing.
