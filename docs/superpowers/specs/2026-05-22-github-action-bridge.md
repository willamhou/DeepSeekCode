# GitHub Action Bridge Slice

**Status:** mode-routing bridge and local background-task delegation landed; benchmark and offline dogfood targets reached
**Parent plan:** `docs/superpowers/plans/2026-05-10-claude-codex-gap-closure-v2.md`

## Gap

Phase 12C calls out the hosted automation gap versus Claude Code / Codex
GitHub workflows. DeepSeekCode already had local `pr review|fix|patch`
commands and GitHub PR context tools, but no Action-facing bridge that could
convert GitHub event payloads into a PR review run.

## Implemented

- New CLI route:
  - `deepseek github action`
  - `--event` / `--event-path` override `GITHUB_EVENT_PATH`
  - `--event-name` overrides `GITHUB_EVENT_NAME`
  - `--trigger` defaults to `@deepseek`
  - `--mode auto|review|fix|patch` selects the delegated PR workflow
  - `--job` is passed through to `deepseek pr fix`
  - `--commit` is passed through to `deepseek pr patch`
  - `--post` asks the existing `deepseek pr review` path to publish a PR
    summary comment when mode resolves to review
  - `--dry-run` parses the GitHub event and prints the resolved target without
    calling the model or posting to GitHub
  - `--require-mode <mode>` fails the action target resolution unless the final
    mode is one of the allowed modes; values may be repeated or comma-separated
  - `--github-output` appends the dry-run target fields to `$GITHUB_OUTPUT` for
    workflow step outputs
  - `--allow-untriggered` lets comment events run without trigger text
  - `--background-task` creates a `deepseek task start` worktree/record for the
    resolved PR request instead of running `deepseek pr review/fix/patch`
    inline
  - `--task-id <id>` gives that background task a stable workflow id
  - `--task-no-run` prepares the worktree/record without launching the model,
    for local workflow gates and deferred execution
- New PR-head resolver route:
  - `deepseek github pr-head <reference>`
  - calls `gh pr view --json headRepositoryOwner,headRefName`
  - converts `owner/repo#123` references to `gh pr view 123 --repo owner/repo`
  - `--repo-owner <owner>` refuses fork-owned PR branches before checkout
  - `--github-output` appends `head_owner` and `head_ref` to `$GITHUB_OUTPUT`
  - `--json-file <path>` lets tests and local smoke runs validate the same
    parser without calling GitHub
- New local no-network workflow gate:
  - `deepseek github fixture-smoke`
  - resolves review and write triggers from fixture event payloads
  - verifies `--require-mode fix|patch` style write routing
  - verifies same-owner PR-head resolution and fork-owned branch refusal
  - creates a temporary bare remote plus work repository, commits a fixture
    write on the PR head branch, pushes it back, and verifies the remote head SHA
  - exercises `github action --background-task --task-no-run` in a temporary git
    repo and verifies task record/worktree creation
- Supported event mapping:
  - `pull_request` / `pull_request_target` -> PR number from `pull_request`
  - `issue_comment` -> PR issue number, only when `issue.pull_request` exists
  - `pull_request_review` -> PR number from `pull_request`
  - `pull_request_review_comment` -> PR number from `pull_request`
- Comment/review events require the configured trigger text unless explicitly
  overridden.
- `--mode auto` routes `@deepseek fix ...` to `PrAction::Fix`, `@deepseek patch
  ...` to `PrAction::Patch`, and plain `@deepseek` / pull request events to
  `PrAction::Review`.
- The bridge delegates to the existing `PrAction::Review`, `PrAction::Fix`, and
  `PrAction::Patch` implementations, so it reuses the same PR fetching, prompt
  construction, CI-log lookup, review body formatting, patch flow, and
  `gh pr comment` delivery path instead of creating a parallel reviewer.
- When `--background-task` is set, the bridge delegates to the repo-local
  background worktree runner instead. The task prompt carries event, mode,
  trigger, post/commit, and optional CI job context, then leaves review/fix/patch
  output in the isolated task worktree for `deepseek task diff/merge/reject`.
- Added disabled-by-default workflow example:
  - `.github/workflows/deepseek-code-review.yml`
  - guarded by repository variable `DEEPSEEK_CODE_REVIEW_ENABLED=true`
  - requires `DEEPSEEK_API_KEY`
  - pins `--mode review` as the safe default because local `fix` / `patch`
    modes require a PR-head checkout before mutating files
  - grants `contents: read`, `pull-requests: write`, and `issues: write`
- Added disabled-by-default write workflow example:
  - `.github/workflows/deepseek-code-write.yml`
  - guarded by repository variable `DEEPSEEK_CODE_WRITE_ENABLED=true`
  - reacts to `@deepseek fix` / `@deepseek patch` comment and review events
  - resolves the target with `deepseek github action --mode auto --dry-run
    --github-output --require-mode fix --require-mode patch`
  - resolves the PR head and refuses fork-owned PR branches through
    `deepseek github pr-head`
  - checks out the same-repository PR head, runs `deepseek github action
    --mode auto`, and commits/pushes resulting changes back to the PR branch
- Added user docs in `docs/github-action.md`.
- Extended the benchmark manifests with GitHub Action bridge coverage:
  - action-labeled review comment planning case
  - action-labeled `@deepseek fix` JavaScript write/validate fixture
  - action-labeled `@deepseek patch` Rust write/validate fixture
  - action-labeled exact replacement request fixture for
    `@deepseek patch change ... becomes ...`
  - default `.dscode/benchmarks.txt` now has `26` `pr_workflow` cases,
    exceeding the Phase 12C benchmark-count target.

## Verification

- `cargo test github_action --lib`
- `cargo test cli::commands::github --lib`
- `cargo test cli::commands::help --lib`
- `cargo test cli_from_argv_routes_github_action_subcommand --lib`
- `cargo test cli_from_argv_parses_github_action_output_and_required_modes --lib`
- `cargo test cli_from_argv_parses_github_action_background_task_flags --lib`
- `cargo test github_background_task_prompt_includes_action_context --lib`
- `cargo test cli_from_argv_parses_github_pr_head_subcommand --lib`
- `cargo test cli_from_argv_parses_github_fixture_smoke_subcommand --lib`
- `cargo test default_manifest_includes_github_action_bridge_pr_workflow_cases --lib`
- `cargo test offline_planner_ --lib`
- `cargo test skills::resolver --lib`
- `cargo run --quiet -- benchmark`
  - current result: `82/82`
  - PR workflow planner/action/comment-plan cases are green
  - Python `pytest`/`uv run pytest` fixtures are covered by `run_shell` fallback
  - Go write-validate fixtures are covered by user-level Go toolchain discovery
  - live gate passes after offline dogfood coverage reached `runs=20`
- `cargo run --quiet -- benchmark --case fixture-github-action-patch-trigger-exact-replacement-rust-mini --out /tmp/deepseek-hosted-exact-patch-benchmark.md`
  - targeted result: `1/1`
  - filtered run did not update benchmark history
- `cargo run --quiet -- github action --event <tmp-event> --event-name issue_comment --dry-run --trigger @deepseek`
- `cargo run --quiet -- github pr-head owner/repo#11 --repo-owner owner --github-output --json-file <tmp-pr-json>`
- `cargo run --quiet -- github fixture-smoke --json`
- `.github/workflows/ci.yml` runs `deepseek github fixture-smoke --json`
  against Linux/macOS/Windows debug binaries.
- `.github/workflows/release.yml` runs `deepseek github fixture-smoke --json`
  against each release-matrix binary before packaging.
- `cargo run --quiet -- dogfood replay-benchmark --category pr_workflow --limit 12 --benchmark-gate`
  - added 12 offline `pr_workflow` replay records
  - action-labeled `@deepseek fix` and `@deepseek patch` cases passed
  - JS/Rust/Python/Go PR repair, retry validate, second-round feedback, and
    Go patch validate cases passed
  - ledger `pr_workflow` slice is now `14/14` success
- `cargo run --quiet -- dogfood replay-benchmark --category recovery --limit 3 --benchmark-gate`
  - filled the live-gate coverage requirement after the `pr_workflow` replay
    batch raised total ledger runs above the coverage threshold
  - final post-replay benchmark gate passed `82/82`
  - live gate passed against the previous dogfood snapshot, `runs 5 -> 20`
- `cargo check --all-targets`
- `cargo fmt --check`
- `git diff --check`
- dry-run samples for `@deepseek fix`, `@deepseek patch`, and explicit
  `--mode review`
- YAML parse checks for both workflow examples
- Hosted workflow evidence:
  - PR #10 proved the hosted write bridge and produced
    `6fd5010 deepseek: apply requested PR update` from GitHub Actions.
  - PR #11 repeated the same write flow after the fixes were merged to the
    default branch and produced `f0fe9a7 deepseek: apply requested PR update`.
  - PR #12 removed the temporary evidence fixtures after the proof was
    preserved in PR history.

## Remaining

- Promote or schedule a stable periodic hosted workflow smoke if this should
  stay continuously monitored instead of relying on PR #10/#11 evidence.
- Promote the refreshed benchmark report/history once the surrounding worktree
  is ready and a full unfiltered run is desired.
- Collect online/model-backed `pr_workflow` dogfood evidence for the new
  action-labeled cases. The current `14/14` `pr_workflow` replay evidence uses
  offline transport and is useful for deterministic coverage, not a substitute
  for model-backed proof.
