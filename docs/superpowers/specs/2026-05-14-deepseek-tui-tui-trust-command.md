# DeepSeek-TUI Parity: TUI Trust Command

## Context

DeepSeek-TUI exposes `/trust [on|off|add <path>|remove <path>|list]` for
workspace-scoped trust mode and explicit external path allowlisting. DeepSeekCode
already had conservative workspace path checks in several write/media/MCP tools,
but the TUI did not provide a first-class trust surface or a persisted
per-workspace trusted path list.

## Goals

- Add a workspace-scoped trust store at `~/.config/dscode/workspace-trust.json`
  with one record per canonical workspace.
- Support TUI `trust` / `/trust` commands:
  - `trust` / `/trust` shows trust mode and trusted external paths.
  - `trust list` / `/trust list` lists the same workspace trust state.
  - `trust on|off` persists all-path trust mode for this workspace.
  - `trust add <path>` persists an existing trusted external path.
  - `trust remove <path>` removes one trusted path.
- Route local file, document, vision, and MCP workspace path resolution through
  the trust store so trusted paths have behavioral effect.
- Keep default behavior safe: untrusted absolute or escaping paths remain
  rejected.

## Acceptance

- TUI queues trust actions before custom slash fallback.
- Local TUI action mutates the trust store and renders a Trust detail panel.
- Trust entries are scoped by canonical workspace path.
- Trusted external paths and trust mode are honored by shared workspace path
  resolution.
- Tests cover parser/queuing, local action handling, trust persistence, and
  trusted/untrusted path resolution.
