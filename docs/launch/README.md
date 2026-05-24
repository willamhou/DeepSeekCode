# Launch Kit

This directory contains public-beta launch copy and checklists for
DeepSeekCode. Keep these materials aligned with
[current status](../current-status.md): Linux/macOS local CLI dogfooding is the
main claim; npm registry publishing and broader hosted product evidence are not
live yet.

## Primary Message

DeepSeekCode is a DeepSeek-first terminal code agent for local repository work.
It can inspect a repo, edit files, run commands, review diffs, keep sessions,
and stay inside the same terminal loop.

Use this concise public-beta claim:

> DeepSeekCode v0.1.3 is a public-beta, DeepSeek-first code-agent CLI for
> Linux/macOS. It ships GitHub Release binaries, Linux arm64 support, verified
> Homebrew install, GHCR image, and release-smoke evidence for the local
> terminal coding loop.

## Links

- Repository: <https://github.com/willamhou/DeepSeekCode>
- Release: <https://github.com/willamhou/DeepSeekCode/releases/tag/v0.1.3>
- Install guide: <https://github.com/willamhou/DeepSeekCode/blob/main/docs/install.md>
- Current status: <https://github.com/willamhou/DeepSeekCode/blob/main/docs/current-status.md>
- Public beta guide: <https://github.com/willamhou/DeepSeekCode/blob/main/docs/public-beta.md>

## Recommended Launch Order

1. Record or refresh the short terminal demo with [demo-script.md](./demo-script.md).
2. Upload a repository social preview image in GitHub repository settings.
3. Post the GitHub release or repository link on personal channels.
4. Post a technical feedback thread with [hacker-news.md](./hacker-news.md).
5. Post Chinese community copy from [chinese-community.md](./chinese-community.md).
6. Use [social-posts.md](./social-posts.md) for X, LinkedIn, Discord, Slack, and
   follow-up posts.
7. Treat [product-hunt.md](./product-hunt.md) as a later launch unless npm and
   richer launch media are ready.

## Preflight

Run these before a meaningful public push:

```bash
cargo fmt --check
cargo test --lib -- --test-threads=1
node scripts/check-secrets.js
deepseek quickstart --json
deepseek update release-smoke --version 0.1.3 --json
```

Also verify:

- README Quick Start still points to the latest release.
- Homebrew install works from a clean macOS machine:
  `brew tap willamhou/deepseekcode && brew install deepseek`.
- `docs/current-status.md` still reflects the latest release evidence.
- Known caveats are visible: npm is not live, Windows is not the current public
  beta focus, and richer GIF/MP4 launch media can still be added.

## Tone

Be direct and specific. Ask for terminal workflow feedback, issue reports, and
real repo dogfooding. Do not describe the project as fully equivalent to Claude
Code or Codex CLI; it is a DeepSeek-first public beta with a comparable
terminal-first workflow.
