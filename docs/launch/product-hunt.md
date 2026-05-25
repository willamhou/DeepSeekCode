# Product Hunt Preparation

Product Hunt is better as a second launch after the current public beta gets
more developer feedback. Use this file to prepare assets, but do not treat
Product Hunt as the immediate next step until launch media includes a short
video-friendly cut.

## Go / No-Go

Go when:

- npm is published and `npx @deepseek-code/cli version` works from a clean
  machine.
- A 30-60 second demo video exists, ideally from the 2048 recorder in
  `docs/demo/record-2048-demo.sh`.
- At least three clean screenshots or gallery images exist.
- README, install docs, and current status match the latest release.
- Known limitations are visible and defensible.

No-go when:

- install requires cloning the repo for most users;
- npm registry install regresses or package install fails on clean machines;
- no one can try the product without waiting for access;
- the launch page only describes the project instead of showing it working.

## Tagline Options

```text
DeepSeek-first terminal code agent
```

```text
A local code-agent CLI for DeepSeek users
```

```text
Terminal-first coding agent for Linux and macOS
```

## Short Description

```text
DeepSeekCode is a public-beta terminal code agent for Linux/macOS. It helps you inspect repos, edit files, run checks, review diffs, and keep local coding-agent sessions in one terminal.
```

## Maker Comment Draft

```text
I built DeepSeekCode because I wanted a terminal-first code-agent workflow for DeepSeek users, closer to Claude Code / Codex CLI than to a plain chat wrapper.

The current public beta focuses on Linux/macOS local repo work: inspect a repository, edit files, run commands, review diffs, and continue from the same terminal session.

v0.1.4 includes npm/npx packages, GitHub Release binaries, Linux arm64 support, a verified Homebrew tap, GHCR publishing, release-smoke checks, and model-backed demo evidence in the README.

It is still early. Windows is not the current public-beta focus, and I am looking for feedback from people who already use terminal-first coding agents.
```

## Gallery Ideas

1. npm/npx install and quickstart.
2. DeepSeekCode generating the playable 2048 app from an empty repo.
3. Browser gameplay of the generated 2048 app.
4. Full-screen TUI in a local repo.
5. `git diff` after the agent edit.
6. Release evidence: GitHub Release, Homebrew Smoke, and GHCR.
