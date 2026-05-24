# DeepSeekCode

[English](./README.md) | [中文](./README.zh-CN.md) | [日本語](./README.ja-JP.md)

DeepSeekCode is a DeepSeek-first terminal code agent for the local development
loop: inspect a repository, edit files, run checks, review the diff, and keep
working from the same terminal.

> Public beta status: usable today for Linux/macOS dogfooding and repository
> work. `v0.1.3` ships GitHub Release binaries, a verified GHCR image, TUI and
> service smoke gates, `deepseek quickstart`, and a release-binary smoke
> verifier. Homebrew, npm registry publishing, broader external repo evidence,
> and richer launch media are still product-hardening work.

<p align="center">
  <img src="./docs/demo/deepseek-code-tui-demo.svg" alt="DeepSeekCode animated TUI demo recording" width="100%">
</p>

<p align="center">
  <strong>Real model-backed edit and test loop</strong><br>
  <img src="./docs/demo/deepseek-code-model-demo.svg" alt="DeepSeekCode real model-backed demo: failing Rust test fixed and validated" width="100%">
</p>

## Why It Exists

DeepSeekCode is meant to feel closer to Claude Code CLI or Codex CLI than to a
plain chat wrapper. The default path is terminal-first and repo-aware:

- `deepseek` opens the full-screen coding-agent TUI in a real TTY.
- `deepseek chat` keeps the line-oriented REPL available.
- `deepseek run` executes one-shot coding tasks.
- Sessions, threads, events, tasks, usage, and automations are persisted under
  `.dscode/runtime/`.
- File tools, patching, diff review, rollback snapshots, todos, hooks, skills,
  subagents, diagnostics, MCP/ACP, and local runtime APIs share the same
  permission and recovery paths.
- Shell work supports foreground commands, background jobs, replay, bounded
  interactive attach, stdin, resize metadata, cancellation, and a local
  shell-supervisor bridge.

## Quick Start

Install from source:

```bash
cargo install --git https://github.com/willamhou/DeepSeekCode.git --locked
deepseek version
deepseek quickstart
deepseek doctor --json
```

Or download a release archive:

```bash
deepseek update download-plan --version 0.1.3
curl -L -o deepseek-linux-x64.tar.gz \
  https://github.com/willamhou/DeepSeekCode/releases/download/v0.1.3/deepseek-linux-x64.tar.gz
curl -L -o deepseek-linux-x64.tar.gz.sha256 \
  https://github.com/willamhou/DeepSeekCode/releases/download/v0.1.3/deepseek-linux-x64.tar.gz.sha256
shasum -a 256 -c deepseek-linux-x64.tar.gz.sha256
tar -xzf deepseek-linux-x64.tar.gz
./deepseek version
```

Or run the published container:

```bash
docker run --rm ghcr.io/willamhou/deepseekcode:0.1.3 version
```

For a local checkout:

```bash
cargo install --path .
deepseek quickstart
deepseek config init
printf '%s\n' '<api-key>' | deepseek config auth DEEPSEEK_API_KEY --stdin
deepseek doctor --json
```

Run a coding task:

```bash
deepseek
deepseek chat
deepseek run "explain the current repository structure"
```

Start the local runtime and connect the TUI:

```bash
deepseek serve --http --addr 127.0.0.1:13000
deepseek tui --runtime-url http://127.0.0.1:13000
```

Set `DEEPSEEK_API_KEY` for real model calls. Local `.env` files are ignored by
git.

## What Works

- Full-screen TUI with Plan / Agent / YOLO modes, approval modals, command
  palette, setup/onboarding, provider/model picker, and MCP management.
- REPL with raw-mode line editing, history, session list/load completion,
  SIGINT cancellation, `/save`, `/load`, `/sessions`, and custom slash commands.
- OpenAI-compatible single and same-turn batch tool calls, routed through the
  normal hook, permission, and recovery layers.
- Guided first-run checks through `deepseek quickstart` and
  `deepseek quickstart --json`.
- Local HTTP/SSE runtime, ACP stdio adapter, MCP client/server surfaces, and
  side-effect tooling behind explicit trust/approval controls.
- RLM helpers for recursive and long-input analysis, model-session context,
  live queue status, event replay, cancellation, recovery, and drain controls.
- CI-smoked Linux/macOS/Windows entrypoints plus release assets for Linux x64,
  Linux arm64, macOS x64, macOS arm64, and Windows x64.
- Verified model-backed README demo and online multi-file external fixture
  evidence for the current release-readiness path.

## Current Limits

For the Linux/macOS local CLI milestone, the core interaction loop is already in
place. The remaining gaps are mainly evidence depth and distribution polish:

- next-release matrix evidence for release-binary shell/runtime smoke, with
  `deepseek update release-smoke --version <version>` available for local
  rechecks;
- Homebrew tap credentials and public tap installation verification;
- optional additional external repo fixtures beyond the Python invoice sample;
- optional GIF/MP4 launch media beyond the committed model-backed SVG.

Windows long-tail service proof, hosted IDE evidence, and npm registry
publishing are broader product-hardening work. They are not blockers for the
Linux/macOS local code-agent CLI milestone.

## Evidence

Useful local checks:

```bash
cargo fmt --check
cargo test --lib -- --test-threads=1
node scripts/check-secrets.js
deepseek quickstart --json
deepseek update publish-status --json
deepseek update release-smoke --version 0.1.3 --json
deepseek tui --entrypoint-smoke --smoke-bin "$(command -v deepseek)"
```

For release and dogfood evidence, see:

- [Release checklist](./docs/release.md)
- [Dogfood evidence](./docs/dogfood-evidence.md)
- [Current status](./docs/current-status.md)

## Documentation

- [Install](./docs/install.md)
- [Public beta guide](./docs/public-beta.md)
- [Current status and roadmap](./docs/current-status.md)
- [Release checklist](./docs/release.md)
- [Dogfood evidence](./docs/dogfood-evidence.md)
- [Demo assets](./docs/demo/README.md)
- [Architecture](./docs/architecture.md)
- [Runtime contract](./docs/runtime.md)
- [TUI workbench](./docs/tui.md)
- [REPL mode](./docs/repl.md)
- [Agent tasks](./docs/agents.md)
- [Skills and profiles](./docs/skills-and-profiles.md)
- [PR / CI integration](./docs/pr-integration.md)
- [Roadmap](./docs/roadmap.md)
- [Changelog](./CHANGELOG.md)

## Repository Notes

This repository is public for transparency and collaboration. Public visibility
does not imply a separate open-source grant beyond the terms in
[LICENSE](./LICENSE).

Do not commit local credentials, API keys, runtime state, or private `.env`
files. The tracked examples use placeholders only.
