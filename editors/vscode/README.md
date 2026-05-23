# DeepseekCode VS Code Extension

VS Code entrypoint for the `deepseek` CLI.

The extension adds a `DeepseekCode` Explorer view, a native agent panel, status bar action, editor title action, command palette commands, and editor context menu entries for common workflows.

## Commands

- `DeepseekCode: Quick Action` opens a quick-pick menu for common workflows
- `DeepseekCode: Open Agent Panel` focuses the sidebar task panel
- `DeepseekCode: Open Chat` launches `deepseek`
- `DeepseekCode: Run Task` prompts for a task and runs `deepseek run`
- `DeepseekCode: Explain Selection` sends the active file and selected text as task context
- `DeepseekCode: Explain Diagnostics` sends active file diagnostics as task context
- `DeepseekCode: Show Active Diff` opens a VS Code diff between `HEAD` and the active editor content
- `DeepseekCode: Run Benchmark` runs `deepseek benchmark`
- `DeepseekCode: Show Dogfood Report` runs `deepseek dogfood report --limit 10`

## Explorer View

The `DeepseekCode` view in the Explorer sidebar exposes the same core actions as clickable items, so common agent workflows are available without opening the command palette.

## Agent Panel

The `DeepseekCode Agent` sidebar panel accepts a task prompt and runs `deepseek exec --json` directly from the webview host. It streams assistant output, tool calls, permission requests, tool results, stderr, and completion status into the panel. Panel tasks include the active editor path, selected text, active-file diagnostics, dirty-buffer status, and a short Git diff/status summary when available.

Use `Resume Latest` to continue the most recent `deepseek exec` runtime session, with the optional task box text as the follow-up prompt. Use `Cancel` in the panel to terminate the active panel task. After a run, use `Review Diff` to open VS Code's diff view for the active file, `Accept` to mark the active-file diff as reviewed in the panel, `Revert File` to discard the active file after confirmation, and `Validate` to run a workspace command with output captured in the panel.

Use `Workspace Changes` to load the Git changed-file queue. Each queued file can be opened in a VS Code diff, marked reviewed, or reverted after confirmation. Untracked files are never removed automatically.

Use `Generated Patches` to inspect unified diffs produced by assistant final output, tool results, or captured `apply_patch` tool calls. Pending generated patches can be opened, applied after `git apply --check`, or rejected from the panel. Single-file generated patches open as a VS Code diff; multi-file patches open as diff text.

The panel also exposes chat, explain, diagnostics, diff, benchmark, and dogfood actions.

## Settings

- `deepseek.command`: command used to launch the CLI. Default: `deepseek`
- `deepseek.maxSelectionChars`: maximum selected text included in task prompts. Default: `6000`

For local development, open this folder in VS Code and run the extension host.

Syntax smoke:

```bash
node --check editors/vscode/extension.js
node editors/vscode/test-extension-smoke.js
```

Headless panel fixture with a mocked DeepseekCode binary and a temporary Git repo:

```bash
npm --prefix editors/vscode run test:panel-fixture
```

This fixture drives the panel provider through diagnostic context injection, generated patch capture, single-file diff opening, patch apply, workspace change refresh, and validation command pass/fail handling without requiring a VS Code GUI.

Extension-host smoke with a mocked DeepseekCode binary:

```bash
VSCODE_BIN=/path/to/code npm --prefix editors/vscode run test:extension-host
```

If `VSCODE_BIN`, `code`, `code-insiders`, or `codium` is unavailable, the script skips by default. Set `DSCODE_REQUIRE_VSCODE=1` in CI to make that a failure.
