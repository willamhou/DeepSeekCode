# Launch Kit

This directory contains public-beta launch copy and checklists for
DeepSeekCode. Keep these materials aligned with
[current status](../current-status.md): Linux/macOS local CLI dogfooding is the
main claim; `v0.1.5` is the npm/npx distribution push, and broader hosted
product evidence is not live yet.

## Primary Message

DeepSeekCode is a DeepSeek-first terminal code agent for local repository work.
It can inspect a repo, edit files, run commands, review diffs, keep sessions,
and stay inside the same terminal loop.

Use this concise public-beta claim:

> DeepSeekCode v0.1.5 is a public-beta, DeepSeek-first code-agent CLI for
> Linux/macOS. It ships verified npm/npx install paths, GitHub Release binaries, Linux arm64
> support, verified Homebrew install, GHCR image, and release-smoke evidence for
> the local terminal coding loop.

## Links

- Repository: <https://github.com/willamhou/DeepSeekCode>
- Release: <https://github.com/willamhou/DeepSeekCode/releases/tag/v0.1.5>
- Install guide: <https://github.com/willamhou/DeepSeekCode/blob/main/docs/install.md>
- Current status: <https://github.com/willamhou/DeepSeekCode/blob/main/docs/current-status.md>
- Public beta guide: <https://github.com/willamhou/DeepSeekCode/blob/main/docs/public-beta.md>

## Recommended Launch Order

1. Review the committed README demo media, including the interactive 2048
   terminal SVG and gameplay GIF/MP4, and refresh only if the UI or positioning
   changed materially.
2. Upload a repository social preview image in GitHub repository settings.
3. Post the GitHub release or repository link on personal channels.
4. Post a technical feedback thread with [hacker-news.md](./hacker-news.md).
5. Post Chinese community copy from [chinese-community.md](./chinese-community.md).
6. Use [social-posts.md](./social-posts.md) for X, LinkedIn, Discord, Slack, and
   follow-up posts.
7. Treat [product-hunt.md](./product-hunt.md) as a later launch after a short
   video-friendly demo cut and broader public-beta feedback are ready.

## Preflight

Run these before a meaningful public push:

```bash
cargo fmt --check
cargo test --lib -- --test-threads=1
node scripts/check-secrets.js
npm view @deepseek-code/cli version
npx @deepseek-code/cli version
deepseek quickstart --json
deepseek update release-smoke --version 0.1.5 --json
```

Also verify:

- README Quick Start still points to the latest release.
- Homebrew install works from a clean macOS machine:
  `brew tap willamhou/deepseekcode && brew install deepseek`.
- Linux users can find release archive and source-install paths in the README
  and install guide.
- `docs/current-status.md` still reflects the latest release evidence.
- Known caveats are visible: Windows is not the current public beta focus, and
  hosted product evidence is still broader hardening work.

## Tone

Be direct and specific. Ask for terminal workflow feedback, issue reports, and
real repo dogfooding. Do not describe the project as fully equivalent to Claude
Code or Codex CLI; it is a DeepSeek-first public beta with a comparable
terminal-first workflow.
