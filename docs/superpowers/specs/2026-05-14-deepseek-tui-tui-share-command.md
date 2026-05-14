# DeepSeek-TUI Parity: TUI Share Command

## Context

DeepSeek-TUI exposes `/share` to export the current session transcript as static HTML and upload it as a public GitHub Gist with the `gh` CLI. DeepSeekCode already stores durable thread items, but the TUI had no share/export command for the active transcript.

## Goals

- Add `share` / `/share` command palette and composer support.
- Add `share help` / `/share help` explaining requirements and current-thread metadata.
- Export the active durable thread items as standalone HTML.
- Attempt `gh gist create --public` with the exported HTML file.
- Preserve the local HTML export path when Gist upload fails.
- Reject share in the HTTP-runtime TUI because the command needs local runtime files and local `gh`.

## Design

`TuiAction::ShareSession { thread_id }` keeps the UI command small and lets the local file-backed TUI handler load the thread, optional session, and item timeline from `RuntimeStore`. The renderer escapes HTML content and emits role/status metadata for user, assistant, tool, and other runtime items.

The upload step uses the local `gh` CLI:

```text
gh gist create --public <html-path> --filename session-export.html --desc "DeepSeekCode TUI Session Export"
```

If `gh` is missing, unauthenticated, offline, or otherwise fails, the detail panel still shows the local HTML path so users can inspect or share it manually.

## Acceptance

- `/share` queues a share action for the selected active thread.
- `/share help` renders share requirements in the detail panel.
- Empty threads do not invoke upload and report that there is nothing to share.
- HTML export escapes user-controlled transcript content.
- HTTP-runtime TUI rejects share as local-only.
- Tests cover command routing, empty-share handling, and HTML escaping.
