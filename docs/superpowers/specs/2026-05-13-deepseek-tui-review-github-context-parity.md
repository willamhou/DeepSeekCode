# DeepSeek-TUI Review GitHub Context Parity

## Gap

DeepSeekCode has `github_pr_context` for GitHub PR metadata/diff and `review`
for local files/diffs, but the two tools were not connected. `review` rejected
remote PR URLs and only told the model to gather GitHub context first, leaving
the model to manually interpret the PR context instead of reusing the structured
review pipeline.

## Target

- Keep GitHub fetching inside `github_pr_context`.
- Let `review` accept `github_pr_context include_diff=true` output through
  `github_context` or `pr_context`.
- Treat PR context with a `diff:` section as a diff review so added-line markers
  and behavioral diff signals still apply.
- Keep remote PR URLs without supplied context rejected, so `review` does not
  gain network or GitHub responsibilities.

## Acceptance Criteria

1. `review target=github_pr_context github_context=<context>` accepts
   `meta.kind=pr` context with a patch diff.
2. The output source kind is `github_pr_diff` when a diff is present.
3. Diff findings include file paths and existing deterministic/behavioral
   markers.
4. Remote PR URLs without `github_context` are still rejected with guidance to
   use `github_pr_context` first.
5. OpenAI/Anthropic review schema exposes `github_context` and `pr_context`.
6. Runtime docs and the parity plan describe the two-tool remote PR review loop.
7. Focused tests and full Rust gates pass.

## Verification

- `/home/willamhou/.cargo/bin/cargo test review_accepts_github_pr_context_with_diff`
- `/home/willamhou/.cargo/bin/cargo test review_rejects_remote_pr_url_without_context`
- `/home/willamhou/.cargo/bin/cargo test build_tool_specs_include_review`
- `/home/willamhou/.cargo/bin/cargo test review`
- `/home/willamhou/.cargo/bin/cargo test`
- `/home/willamhou/.cargo/bin/cargo fmt --check`
- `git diff --check`
- `/home/willamhou/.cargo/bin/cargo package --allow-dirty`
