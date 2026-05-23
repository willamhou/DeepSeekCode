# VS Code Native Agent Panel Slice

**Status:** headless diagnostic patch fixture landed
**Parent plan:** `docs/superpowers/plans/2026-05-10-claude-codex-gap-closure-v2.md`

## Gap

The Phase 12B audit calls out VS Code as a high-weight Claude Code / Codex
product gap. Before this slice, the extension had useful quick actions, but the
sidebar agent panel still delegated tasks to a VS Code terminal. That meant the
extension could not show an in-panel assistant stream, tool trace, or task
status.

## Implemented

- The `DeepseekCode Agent` webview now launches `deepseek exec --json` directly
  from the panel for task submissions.
- The panel consumes JSONL events from the child process and renders:
  - assistant text deltas
  - reasoning deltas in the log pane
  - tool calls
  - permission requests
  - tool results
  - final assistant output
  - stderr and process completion status
- The panel exposes a cancel button that sends termination to the active child
  process.
- Panel task prompts include best-effort VS Code context:
  - active file path
  - dirty-buffer marker
  - selected text, clipped by `deepseek.maxSelectionChars`
  - active-file diagnostics
  - active-file Git status / short diff summary
- Command-palette terminal tasks now reuse the same richer context builder.
- The panel now has active-file post-run review controls:
  - `Review Diff` opens VS Code's diff view for the active file and refreshes
    the panel diff summary.
  - `Accept` marks the active-file diff as reviewed in panel state without
    mutating files.
  - `Revert File` discards the active file only after a modal confirmation and
    refuses to remove untracked files automatically.
  - `Refresh Diff` updates the active-file summary without opening a diff.
- The panel now has a validation command input. `Validate` runs the command in
  the workspace, streams stdout/stderr into the panel, and reports pass/fail
  status.
- The panel now has `Resume Latest`, which runs `deepseek exec resume --json`
  and optionally uses the current task box text as the follow-up prompt.
- The panel now has a workspace changed-file queue:
  - `Workspace Changes` renders `git status --short` files and a shortstat
    summary.
  - Each file can be opened in a VS Code diff or marked reviewed in panel state.
  - Tracked files can be reverted after modal confirmation.
  - Untracked files are intentionally not removed automatically.
- The panel now has a generated patch artifact queue separate from already
  written Git changes:
  - assistant final messages and tool results are scanned for unified diff
    fences / raw unified diff chunks
  - `apply_patch` tool calls are captured as generated patch artifacts, but
    marked `captured` because the tool path writes to the workspace separately
  - pending generated patches can be opened, applied, or rejected from the panel
  - single-file generated patches open as a VS Code diff by rendering the
    proposed post-patch content in a temporary directory; multi-file patches
    fall back to a diff-language document
  - `Apply` runs `git apply --check -` before mutating the workspace with
    `git apply -`, then refreshes the workspace changed-file queue
- The extension directory now has an extension-host smoke harness:
  - `test-extension-host.js` creates a temporary workspace, writes a mocked
    `deepseek` binary that emits JSONL events, configures `deepseek.command`,
    and launches a VS Code runner with `--extensionDevelopmentPath` /
    `--extensionTestsPath`
  - `test-extension-host-runner.js` runs inside the extension host, activates
    the extension, drives a test panel provider, and asserts assistant output
    plus a pending generated patch artifact
  - the script defaults to skip when `VSCODE_BIN`, `code`, `code-insiders`, or
    `codium` is unavailable; CI can set `DSCODE_REQUIRE_VSCODE=1` to make that
    missing runner fail
- The extension directory now has a headless panel fixture:
  - `test-panel-fixture.js` creates a temporary Git repo with an active file
    diagnostic, drives the real panel provider through a mocked VS Code API,
    and launches a mocked `deepseek exec --json` child process
  - the mocked agent receives panel prompt context and asserts diagnostics were
    injected
  - the fixture captures an assistant-generated unified diff, opens the
    single-file generated patch through the same `vscode.diff` path, applies
    the patch through `git apply --check` + `git apply`, refreshes the workspace
    changed-file queue, and runs a validation command that verifies the file
    content
  - the fixture caught and fixed two real edge cases: generated patches without
    `a/` / `b/` headers now fall back to `git apply -p0`, and workspace
    `git status --short` parsing preserves leading status-column spaces instead
    of trimming them away

## Verification

- `node --check editors/vscode/extension.js`
- `node editors/vscode/test-extension-smoke.js`
- `npm --prefix editors/vscode run test:panel-fixture`
- `node --check editors/vscode/test-extension-host.js`
- `node --check editors/vscode/test-extension-host-runner.js`

## Remaining

This is not yet the full Phase 12B workbench. Remaining IDE parity items are:

- extension-host smoke execution evidence on a machine with a VS Code CLI
- manual GUI fixture evidence for diagnostic -> patch -> diff -> validation
