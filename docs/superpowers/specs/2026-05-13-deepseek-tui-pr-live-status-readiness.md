# DeepSeek-TUI PR Live Status Readiness

## Context

The remaining remote PR gap is not planner behavior: seeded fixtures now cover
context review, semantic review, comment planning, top-level comment retry, and
inline review-comment retry. The weak spot is live verification against a real
GitHub repository, which needs external PRs, authentication, and explicit write
authorization.

## Scope

- Add a non-mutating CLI check for live PR fixture prerequisites.
- Verify `gh` authentication, PR metadata, changed files, and PR diff
  availability.
- Check current branch alignment without blocking read-only review readiness.
- Check repository permissions, and make write readiness explicit behind a
  `--require-write` flag.
- Do not post comments, push commits, checkout branches, or modify files.

## Implementation

- `deepseek pr live-status <pr>` fetches PR metadata/diff through `gh`, reads
  repository permissions through `gh api repos/<owner>/<repo>`, and prints a
  status report.
- `--require-write` fails unless repo permissions include `push`, `maintain`, or
  `admin`.
- `--json` emits the same readiness checks as `deepseek.pr_live_status.v1` for
  CI scripts.
- The command is handled before model configuration loading, so it does not
  require a DeepSeek API key.
- `docs/pr-integration.md` documents the command and clarifies the relationship
  between CLI review posting and guarded inline review-comment tools.

## Verification

- `/home/willamhou/.cargo/bin/cargo test pr --lib`
- `/home/willamhou/.cargo/bin/cargo test github --lib`
- `/home/willamhou/.cargo/bin/cargo fmt --check`
- `deepseek pr live-status <pr> --json` JSON rendering is covered by unit tests;
  live execution still requires an external disposable PR.
- `git diff --check`

## Remaining

Real write-path live fixtures still require a disposable test repository, an
open pull request, and explicit authorization to post and clean up GitHub
comments.
