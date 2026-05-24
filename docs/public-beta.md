# Public Beta Guide

This guide keeps public messaging aligned with what DeepSeekCode can prove
today.

## Positioning

DeepSeekCode is a DeepSeek-first terminal code agent for Linux/macOS local
development. It is useful when someone wants a repo-aware CLI that can inspect
code, edit files, run commands, review diffs, resume sessions, and keep runtime
state locally.

The honest public-beta claim is:

> DeepSeekCode is usable today for Linux/macOS dogfooding and repository work,
> with a full-screen TUI, REPL, durable runtime, permissioned tools, shell/PTY
> workflows, release binaries, a verified Homebrew tap, a model-backed README
> demo, online dogfood evidence, and first-run checks. npm distribution,
> broader external samples, and hosted product evidence are still in progress.

Do not describe the project as fully equivalent to Claude Code CLI or Codex CLI
yet. The local loop is close enough to dogfood, but the public install and
evidence story is still being hardened.

## Who Should Try It

Good public-beta users:

- developers on Linux or macOS who are comfortable with a terminal-first code
  agent;
- users who can provide `DEEPSEEK_API_KEY`;
- contributors willing to report install, TUI, shell, and repo-editing issues;
- people evaluating DeepSeek-backed coding workflows rather than hosted IDE
  integrations.

Less ideal users for this phase:

- users who require npm as the only install path;
- users who need polished hosted IDE flows;
- Windows-only users who expect service-level parity with Linux/macOS.

## First-Run Path

Recommended path for public-beta testers:

```bash
brew tap willamhou/deepseekcode
brew install deepseek
deepseek quickstart
deepseek config init
printf '%s\n' '<api-key>' | deepseek config auth DEEPSEEK_API_KEY --stdin
deepseek doctor --json
deepseek
```

Source install remains available for users who want to build from git:

```bash
cargo install --git https://github.com/willamhou/DeepSeekCode.git --locked
deepseek quickstart
deepseek config init
printf '%s\n' '<api-key>' | deepseek config auth DEEPSEEK_API_KEY --stdin
deepseek doctor --json
deepseek
```

For release archive users, start with:

```bash
deepseek update download-plan --version 0.1.3
deepseek update release-smoke --version 0.1.3 --json
```

Then try a bounded task in an existing repository:

```bash
deepseek run "summarize this repository and identify the main test command"
```

## What To Show

The strongest current proof points are:

- the two README SVG demos, including the model-backed edit/test loop;
- `deepseek quickstart` and `deepseek doctor --json` for first-run readiness;
- CI-smoked TUI entrypoints and service/shell fixtures;
- `deepseek update release-smoke --version 0.1.3 --json` for release binary
  verification on the current platform;
- verified Homebrew tap install on macOS x64 and macOS arm64;
- online Python, Rust, and Node external fixture evidence recorded through
  dogfood tooling;
- reusable Python, Rust, and Node external fixture scaffolds for refreshing or
  extending model-backed evidence when needed.

Use [docs/current-status.md](./current-status.md) for the exact state and
[docs/dogfood-evidence.md](./dogfood-evidence.md) for evidence commands.

## Public Caveats

Keep these caveats visible when promoting the project:

- npm registry publishing is product-hardening work, not the primary install
  path yet.
- Homebrew is published, but future tag automation still needs
  `HOMEBREW_TAP_TOKEN`; `v0.1.3` was updated manually after the tag run.
- More online model-backed runs against larger external fixtures would make the
  evidence base stronger.
- Hosted IDE evidence and broader Windows service proof are outside the current
  Linux/macOS local CLI milestone.
- Rich GIF/MP4 launch media can improve conversion, but the committed SVG demo
  is already enough to show the core loop.

## Promotion Checklist

Before a broader public push:

```bash
cargo fmt --check
cargo test --lib -- --test-threads=1
node scripts/check-secrets.js
deepseek quickstart --json
deepseek update publish-status --json
deepseek update release-smoke --version 0.1.3 --json
```

Also check:

- README quick-start commands still match the latest release.
- [docs/install.md](./install.md) covers the supported install path.
- [docs/current-status.md](./current-status.md) has a current date.
- [docs/demo/README.md](./demo/README.md) can regenerate the committed SVGs.
- Known gaps are described as caveats, not hidden behind vague language.

## Short Copy

Short project description:

> DeepSeekCode is a DeepSeek-first terminal code agent for Linux/macOS. It gives
> you a full-screen TUI, REPL, repo-aware tools, permissioned shell workflows,
> durable local runtime state, and release evidence for real code editing loops.

Longer public-beta copy:

> DeepSeekCode is a public-beta terminal code agent built around DeepSeek and
> local repository work. It can inspect files, apply patches, run checks, review
> diffs, resume sessions, and drive shell workflows from the terminal. The
> Linux/macOS local CLI loop is ready for dogfooding, and Homebrew is verified
> for macOS installs; npm distribution, broader external repo samples, hosted
> IDE evidence, and richer launch media are still being hardened.
