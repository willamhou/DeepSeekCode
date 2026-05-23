# Historical MVP

This page records the original v0.1 target. It is historical context, not the
current product roadmap. For current status and next work, use:

- [Current status](./current-status.md)
- [Roadmap](./roadmap.md)
- [Release checklist](./release.md)

## Original v0.1 Goal

The first milestone was not "the strongest code agent"; it was a stable local
code-editing loop:

> In a local repository, use DeepSeek to read code, edit code, run commands, and
> continue fixing based on validation output.

The primary command is `deepseek`; `dscode` remains only as a compatibility
alias.

## Original Scope

- Start the CLI with `deepseek`.
- Inspect a repository with file listing, file reading, and text search.
- Apply patches instead of overwriting whole files.
- Run approved shell commands.
- Show diffs and save/resume session state.
- Use `doctor` and `smoke` to diagnose local setup.

That milestone is complete and has been superseded by the current Linux/macOS
code-agent CLI goal.

## What Changed Since v0.1

Several items that were explicitly out of scope for the first version now exist
as implemented features or working prototypes:

- multi-provider/model configuration and pickers;
- TUI workbench and REPL line editor;
- durable runtime with HTTP/SSE surfaces;
- MCP/ACP client and server surfaces;
- local skills, remote skill installers, custom slash commands, and hooks;
- subagents and background worktree tasks;
- GitHub Action review/write bridge;
- VS Code native panel prototype;
- release matrix, GHCR image, npm package staging, and Homebrew formula
  rendering.

The current remaining work is therefore not MVP closure. It is release and
product hardening.
