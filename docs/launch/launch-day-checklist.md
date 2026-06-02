# Launch-Day Checklist

Use this as the operator checklist before posting outside GitHub. The goal is a
small, evidence-backed public beta push, not a broad marketing blast.

## 1. Verify The Public Install Paths

```bash
npm view @deepseek-code/cli version
npx @deepseek-code/cli@0.1.6 version
deepseek quickstart --json
deepseek update release-smoke --version 0.1.6 --json
```

Known-good hosted evidence:

- Release Matrix: <https://github.com/willamhou/DeepSeekCode/actions/runs/26802630494>
- Release Smoke: <https://github.com/willamhou/DeepSeekCode/actions/runs/26803043308>
- Homebrew Smoke: <https://github.com/willamhou/DeepSeekCode/actions/runs/26803227609>
- npm package: <https://www.npmjs.com/package/@deepseek-code/cli>

## 2. Verify The Landing Surface

- README first screen shows the install path, public beta status, and 2048 demo.
- README links to `docs/evidence.md`, `docs/current-status.md`, and
  `docs/install.md`.
- `README.zh-CN.md` and `README.ja-JP.md` point to `v0.1.6`, not an older
  release.
- The 2048 terminal SVG and gameplay GIF/MP4 render from GitHub.

## 3. Post In This Order

1. GitHub release or repository link on personal channels.
2. Show HN using [hacker-news.md](./hacker-news.md).
3. Chinese technical communities using [chinese-community.md](./chinese-community.md).
4. X, LinkedIn, Discord, or Slack using [social-posts.md](./social-posts.md).
5. DeepSeek integration list PR using
   [deepseek-integration-pr.md](./deepseek-integration-pr.md).

## 4. Response Rules

- Ask for install friction, first-run UX, TUI approval flow, shell behavior, and
  real-repo editing feedback.
- Keep caveats visible: Linux/macOS is the public beta focus; Windows assets
  exist but service-level proof is not the milestone.
- Link evidence instead of arguing from claims.
- Do not ask for upvotes or cross-post the same text everywhere.
- Open GitHub issues for concrete bug reports and tag them by install path,
  platform, or workflow area.
