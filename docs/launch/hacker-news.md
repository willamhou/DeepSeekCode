# Hacker News Launch Copy

Use this when posting a Show HN or a regular HN link. The preferred link is the
repository, because the README contains installation commands and proof links.

## Title Options

Preferred:

```text
Show HN: DeepSeekCode - a DeepSeek-first CLI code agent
```

Alternative:

```text
Show HN: DeepSeekCode - a terminal code agent for Linux and macOS
```

Avoid version-only titles such as `DeepSeekCode v0.1.3 is out`; the release is
not the story. The story is that people can install and try the terminal agent.

## Link

```text
https://github.com/willamhou/DeepSeekCode
```

## First Comment

```text
Hi HN,

I built DeepSeekCode, a DeepSeek-first terminal code agent for local repository
work. The goal is a Claude Code / Codex CLI-style loop for DeepSeek users:
inspect a repo, edit files, run checks, review the diff, and keep working from
the same terminal.

The current public beta is focused on Linux/macOS. v0.1.3 has GitHub Release
binaries, Linux x64/arm64 and macOS x64/arm64 assets, a verified Homebrew tap,
a GHCR image, and release-smoke checks that download and validate the public
release assets.

Quick try:

brew tap willamhou/deepseekcode
brew install deepseek
deepseek quickstart
deepseek

What works today: full-screen TUI, REPL, one-shot run mode, repo-aware file
tools, shell workflows with approvals, sessions/runtime state, MCP/ACP surfaces,
rollback/diff review paths, and model-backed demo evidence in the README.

Current limits: npm publishing is not live yet, Windows is not the public-beta
focus, and I still want more external repo dogfood evidence and richer launch
media.

I would especially like feedback from people who already use terminal-first
coding agents: install friction, first-run UX, approval flow, shell behavior,
and where DeepSeek-backed workflows feel different in practice.
```

## Reply Guidance

- Answer technical questions with links to the exact docs or release evidence.
- Acknowledge public-beta limits directly.
- Do not ask for upvotes.
- If someone asks about npm, say it is prepared but not published yet because
  the registry token is not configured.
- If someone asks about Windows, say release assets exist, but Linux/macOS local
  CLI dogfooding is the current milestone.
