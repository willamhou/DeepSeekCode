# Demo Script

Use this to record a 30-60 second launch demo. The goal is to show the product
working, not to explain every feature.

## Recording Setup

- Terminal size: about 120x34.
- Theme: high-contrast dark theme with readable font.
- Repo: a small disposable Rust, Node, or Python project.
- Keep the API key out of frame and out of shell history.
- Record the terminal only; avoid showing private browser tabs or local paths
  that should not be public.

## Storyboard

1. Install or verify the CLI.
2. Run first-run checks.
3. Open a small repo.
4. Ask DeepSeekCode to inspect the repo and make a bounded change.
5. Approve file/shell actions.
6. Show the diff.
7. Run tests.
8. Close with the GitHub repo URL.

## Command Flow

```bash
brew tap willamhou/deepseekcode
brew install deepseek
deepseek version
deepseek quickstart
```

In a disposable repository:

```bash
deepseek run "inspect this repository, identify the test command, make one small safe improvement, and run the relevant test"
git diff --stat
git diff
```

If a live model call is too slow for a public recording, use the committed
model-backed SVG in the README and record a shorter install/quickstart clip.

## What To Emphasize On Screen

- `brew install` works.
- `deepseek quickstart` gives first-run confidence.
- The agent reads repo context before editing.
- File changes are reviewable with `git diff`.
- Tests or checks run in the same terminal loop.
- The project is public beta and asks for real terminal-workflow feedback.

## Social Preview Brief

Create a repository social preview image and upload it in GitHub settings.

Recommended content:

```text
DeepSeekCode
DeepSeek-first terminal code agent
Homebrew install | Linux/macOS | Public beta
```

Recommended dimensions:

- 1280x640 PNG or JPG.
- Solid background.
- Large terminal screenshot or terminal-style panel.
- Keep text large enough to read in a link preview.
- Avoid private paths, API keys, or real customer code.
