# Social Posts

Use these for X, LinkedIn, Bluesky, Discord, Slack, or project update channels.

## One-Liner

```text
DeepSeekCode v0.1.3 is a public-beta, DeepSeek-first terminal code agent for Linux/macOS: macOS Homebrew install, Linux release binaries, GHCR, Linux arm64, and release-smoke evidence.
```

## X Thread

```text
DeepSeekCode v0.1.3 public beta is out.

It is a DeepSeek-first terminal code agent for local repository work: inspect files, edit code, run checks, review diffs, and keep the session in one terminal.

macOS install:
brew tap willamhou/deepseekcode
brew install deepseek
deepseek config init
printf '%s\n' '<api-key>' | deepseek config auth DEEPSEEK_API_KEY --stdin
deepseek quickstart
deepseek

Linux users can use the release archive or source install path in the README.

This release includes:
- Linux x64 + Linux arm64
- macOS x64 + macOS arm64
- verified macOS Homebrew tap
- GitHub Release binaries
- GHCR image
- release-smoke checks against public assets

The goal is a Claude Code / Codex CLI-style workflow for DeepSeek users, not a plain chat wrapper.

Current limits:
- npm is prepared but not published yet
- Linux/macOS is the public-beta focus
- more real external repo evidence is still useful

Repo:
https://github.com/willamhou/DeepSeekCode

I am looking for feedback from terminal-first coding-agent users: install friction, approval flow, shell behavior, repo editing, and first-run UX.
```

## LinkedIn / Longer Update

```text
I released DeepSeekCode v0.1.3 as a public beta.

DeepSeekCode is a DeepSeek-first terminal code agent for local repository work. It is built around a terminal-first loop: inspect a repo, edit files, run checks, review diffs, and continue from the same local session.

The v0.1.3 release now has public GitHub Release binaries, Linux x64/arm64 and macOS x64/arm64 assets, a verified macOS Homebrew tap, GHCR image publishing, and release-smoke checks against the published artifacts.

macOS install:

brew tap willamhou/deepseekcode
brew install deepseek
deepseek config init
printf '%s\n' '<api-key>' | deepseek config auth DEEPSEEK_API_KEY --stdin
deepseek quickstart
deepseek

Linux users can use the release archive or source install path documented in the README.

This is still a public beta. npm publishing is not live yet, Windows is not the current focus, and I want more real-world feedback from Linux/macOS terminal workflows.

Repo:
https://github.com/willamhou/DeepSeekCode
```

## Discord / Slack

```text
I am dogfooding DeepSeekCode v0.1.3, a DeepSeek-first terminal code agent for Linux/macOS.

macOS install:
brew tap willamhou/deepseekcode
brew install deepseek
deepseek config init
printf '%s\n' '<api-key>' | deepseek config auth DEEPSEEK_API_KEY --stdin
deepseek quickstart
deepseek

Linux: use the release archive or source install path in the README.

Repo: https://github.com/willamhou/DeepSeekCode

Feedback I am looking for: install friction, first-run UX, approval flow, shell behavior, and whether the terminal coding loop feels useful on real repos.
```
