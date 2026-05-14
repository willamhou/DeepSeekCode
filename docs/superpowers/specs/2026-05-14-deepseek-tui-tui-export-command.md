# DeepSeek-TUI Parity: TUI Export Command

## Context

DeepSeek-TUI exposes `/export [path]` to write the current chat history to a
Markdown file. DeepSeekCode already persists durable thread items and recently
added `/share` for HTML/Gist export, but it still lacked the local Markdown
export command that users expect from DeepSeek-TUI.

## Goals

- Add `export` / `/export` command palette and composer support.
- Add optional `export <path>` / `/export <path>` path selection.
- Add `export help` / `/export help` explaining path resolution and active
  thread metadata.
- Write Markdown for the active durable thread transcript.
- Default to `chat_export_<timestamp>.md` in the selected workspace when no
  path is supplied.
- Resolve relative paths under the selected workspace; allow absolute and
  `~/...` paths.
- Reject export in HTTP-runtime TUI mode because it writes local files from the
  TUI process.

## Design

`TuiAction::ExportThread { thread_id, path }` keeps UI parsing separate from
filesystem writes. The local file-backed TUI handler loads the active thread,
optional session, and item timeline from `RuntimeStore`, renders a Markdown
document with session/thread/model/workspace metadata, and writes it to the
resolved path after creating the parent directory.

The Markdown renderer maps common runtime roles to DeepSeek-TUI-style labels:
`**You:**`, `**Assistant:**`, `*System:*`, `**Tool:**`, and `*Thinking:*`.
Unknown item types are title-cased and preserved with per-item type/status
metadata.

## Acceptance

- `/export` queues a Markdown export action for the selected active thread.
- `/export <path>` preserves the requested path on the queued action.
- `/export help` renders path rules in the detail panel.
- Local file-backed TUI writes Markdown containing header metadata and
  user/assistant transcript content.
- Empty threads produce a Markdown file with a clear empty-transcript note.
- HTTP-runtime TUI rejects export as local-only.
- Tests cover command routing, HTTP rejection, Markdown writing, and empty
  transcript rendering.
