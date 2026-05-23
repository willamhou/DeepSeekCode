# Roadmap

Last updated: 2026-05-23

This page is the current product roadmap. Older phase-by-phase execution notes
live in `docs/superpowers/` and in git history; those historical notes are useful
for audit trails, but they are not the source of truth for current status.

## Current Position

DeepSeekCode is now usable for Linux/macOS dogfooding and repository work:

- bare `deepseek` opens the full-screen TUI in a real terminal;
- `deepseek quickstart` provides a side-effect-free first-run readiness check
  with text and JSON output;
- `deepseek chat` remains available as the line-oriented REPL;
- model-backed tasks can read files, apply patches, run shell checks, inspect
  diffs, and resume from durable runtime state;
- local runtime, shell-supervisor, background worktree tasks, MCP/ACP surfaces,
  GitHub Action bridge, dogfood evidence, and release packaging checks all have
  repeatable gates;
- PR #16 full CI passed Linux, macOS, and Windows:
  https://github.com/willamhou/DeepSeekCode/actions/runs/26334525472
- verified online multi-file external fixture evidence is tracked under
  `.dscode/dogfood/external-fixture-python-invoice-multifile-verification.json`.

The Linux/macOS local code-agent CLI milestone is effectively established. The
remaining work is mostly release hardening, external evidence depth, publishing,
and documentation polish.

## Near-Term Priorities

### 1. Release Hardening For Linux/macOS

- Run the next release matrix and preserve release-binary smoke evidence for
  Linux/macOS TUI entrypoint, shell fixture, service smoke, task worktree smoke,
  GitHub bridge smoke, and multi-file fixture scaffold.
- Keep `deepseek update publish-status --strict` fail-closed on verified online
  dogfood evidence, release assets, npm package artifacts, Homebrew checksums,
  and public install readiness.
- Keep `node scripts/check-secrets.js` in every release path.

### 2. Homebrew And npm Publishing

- Configure `HOMEBREW_TAP_REPOSITORY` and `HOMEBREW_TAP_TOKEN`.
- Publish and verify the generated `Formula/deepseek.rb` against the GitHub
  Release archives and `.sha256` files.
- Configure `NPM_TOKEN` / `NODE_AUTH_TOKEN`.
- Publish platform npm packages and the root wrapper, then verify public
  `npm install` produces a working `deepseek` command.

For the Linux/macOS CLI milestone, Homebrew is higher priority than npm because
it is the most natural install path for macOS users.

### 3. More External Model-Backed Samples

- Keep the Python invoice multi-file fixture as the canonical tracked sample.
- Add one or two more disposable external repo samples only when they cover new
  behavior, such as multi-step recovery, larger diffs, or non-Python/Rust/JS
  workflows.
- Require `dogfood external-evidence` verification with
  `post_validation_passed=true` for every sample counted as release evidence.

### 4. Documentation Compression

- Keep README focused on install, `deepseek quickstart`, current gap, demo, and
  validation.
- Keep `docs/current-status.md` focused on current facts and near-term work.
- Keep `docs/release.md` as the operator checklist.
- Treat `docs/superpowers/` as historical execution logs, not user-facing
  status.

### 5. Broader Product Hardening

- Record installed systemd/launchd service smoke evidence on clean machines.
- Record a real VS Code runner/manual GUI fixture for the native agent panel.
- Continue Windows ConPTY/service validation, while keeping it separate from
  the Linux/macOS local CLI milestone.
- Periodically compare the core loop against Claude Code CLI, Codex CLI, and
  DeepSeek-TUI.

## Current Stop Conditions

For the Linux/macOS local CLI milestone, stop treating new work as blocking once
these are true:

- hosted Linux/macOS CI gates remain green;
- release-binary Linux/macOS smoke evidence exists from the next release matrix;
- at least one verified online multi-file external fixture remains tracked;
- Homebrew public install is either published and verified, or explicitly marked
  blocked on tap credentials;
- README and install docs show `deepseek quickstart` as the accurate first-run
  path.

Everything else belongs to the broader product-hardening backlog.
