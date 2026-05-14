# DeepSeek-TUI Parity: TUI Change Command

## Context

DeepSeek-TUI exposes `/change` to show the latest bundled changelog entry.
DeepSeekCode already ships `CHANGELOG.md`, but the TUI did not have the
DeepSeek-TUI-compatible command entry point.

## Goals

- Add `change` / `/change` command-palette and composer support.
- Add `changes` / `/changes` and `changelog` / `/changelog` aliases.
- Add `change help` / `/change help` detail rendering.
- Extract the first `## ...` version section from the bundled `CHANGELOG.md`.
- Render the latest entry in the right-side detail panel without starting a
  model turn or touching runtime state.
- Bound the rendered changelog section so an unusually large release note does
  not flood the TUI.
- Reject unsupported arguments with a concise usage error.

## Design

The command is handled entirely inside `TuiApp`. It parses to
`TuiChangeCommand`, renders either command help or the latest changelog section,
and stores the result as `TuiMcpDetailKind::Change` detail content. This mirrors
DeepSeek-TUI's local command behavior while keeping DeepSeekCode's durable
runtime untouched.

The section extractor intentionally accepts `## 0.1.0 - date` and
`## [0.1.0] - date` styles so it works with DeepSeekCode and DeepSeek-TUI style
changelogs.

## Acceptance

- `change` / `/change` renders the latest bundled changelog entry.
- `changes` / `/changes` and `changelog` / `/changelog` behave as aliases.
- `change help` / `/change help` renders usage and aliases.
- Invalid arguments show `usage: change or /change; use change help for details`.
- Tests cover command rendering, help aliases, invalid arguments, and latest
  changelog-section extraction.
