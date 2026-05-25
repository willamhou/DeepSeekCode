# Social Posts

Use these for X, LinkedIn, Bluesky, Discord, Slack, or project update channels.

## One-Liner

```text
DeepSeekCode v0.1.5 is a public-beta, DeepSeek-first terminal code agent for Linux/macOS: npm/npx install, Homebrew, release binaries, GHCR, Linux arm64, and release-smoke evidence.
```

## X Thread

```text
DeepSeekCode v0.1.5 public beta is out.

It is a DeepSeek-first terminal code agent for local repository work: inspect files, edit code, run checks, review diffs, and keep the session in one terminal.

The README demo shows a real interactive run where DeepSeekCode creates a playable 2048 game from an empty repo.

npm install:
npm install -g @deepseek-code/cli
deepseek quickstart
deepseek

macOS Homebrew:
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
- npm/npx wrapper packages

Evidence:
- Release Matrix: 26380726246
- Release Smoke: 26380981708
- Homebrew Smoke: 26380934049

The goal is a Claude Code / Codex CLI-style workflow for DeepSeek users, not a plain chat wrapper.

Current limits:
- Linux/macOS is the public-beta focus
- more real external repo evidence is still useful

Repo:
https://github.com/willamhou/DeepSeekCode

I am looking for feedback from terminal-first coding-agent users: install friction, approval flow, shell behavior, repo editing, and first-run UX.
```

## LinkedIn / Longer Update

```text
I released DeepSeekCode v0.1.5 as a public beta.

DeepSeekCode is a DeepSeek-first terminal code agent for local repository work. It is built around a terminal-first loop: inspect a repo, edit files, run checks, review diffs, and continue from the same local session.

The v0.1.5 release now has npm/npx packages, public GitHub Release binaries, Linux x64/arm64 and macOS x64/arm64 assets, a verified Homebrew tap, GHCR image publishing, and release-smoke checks against the published artifacts.

The README also includes a real interactive 2048 demo: DeepSeekCode starts from an empty repo, writes the game, and the generated browser game is played locally.

npm install:

npm install -g @deepseek-code/cli
deepseek quickstart
deepseek

macOS Homebrew:

brew tap willamhou/deepseekcode
brew install deepseek
deepseek config init
printf '%s\n' '<api-key>' | deepseek config auth DEEPSEEK_API_KEY --stdin
deepseek quickstart
deepseek

Linux users can use the release archive or source install path documented in the README.

This is still a public beta. Windows is not the current focus, and I want more real-world feedback from Linux/macOS terminal workflows.

Release evidence:
- Release Matrix: https://github.com/willamhou/DeepSeekCode/actions/runs/26380726246
- Release Smoke: https://github.com/willamhou/DeepSeekCode/actions/runs/26380981708
- Homebrew Smoke: https://github.com/willamhou/DeepSeekCode/actions/runs/26380934049

Repo:
https://github.com/willamhou/DeepSeekCode
```

## Discord / Slack

```text
I am dogfooding DeepSeekCode v0.1.5, a DeepSeek-first terminal code agent for Linux/macOS.

npm install:
npm install -g @deepseek-code/cli
deepseek quickstart
deepseek

macOS Homebrew:
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
