# DeepSeekCode GitHub Action Bridge

DeepSeekCode can run from GitHub Actions through:

```bash
deepseek github action --post --trigger "@deepseek"
```

The command reads `GITHUB_EVENT_PATH` and `GITHUB_EVENT_NAME`, maps supported
GitHub events to a PR reference, and delegates to the existing
`deepseek pr review <repo>#<number>` path. `--post` publishes the review summary
with `gh pr comment`; omit it to print the review locally.

Use `--mode review|fix|patch|auto` to pick the PR path. `review` delegates to
`deepseek pr review`, `fix` delegates to `deepseek pr fix`, and `patch`
delegates to `deepseek pr patch`. `auto` is the default: comment/review bodies
such as `@deepseek fix` and `@deepseek patch` route to the matching local PR
workflow, while plain `@deepseek` or pull request events route to review.

Use `--background-task` when a workflow or operator should delegate the resolved
PR request into the local background worktree runner instead of running the
`deepseek pr ...` command inline. It creates a `deepseek task start` record and
prints the task JSON; the isolated worktree can then be inspected with
`deepseek task show`, `deepseek task diff`, `deepseek task merge`, or
`deepseek task reject`.

```bash
deepseek github action --mode auto --background-task --task-id "gha-${GITHUB_RUN_ID}"
```

`--task-no-run` creates only the task record and worktree, which is useful for
local workflow smoke tests or workflows that want to prepare a task for a later
runner. `--task-id <id>` gives the task a stable path-safe id.

Supported events:

- `pull_request` / `pull_request_target`: reviews the PR without a comment trigger
- `issue_comment`: requires the issue to be a PR and the comment body to contain
  the trigger text
- `pull_request_review`: requires the review body to contain the trigger text
- `pull_request_review_comment`: requires the comment body to contain the
  trigger text

For a dry parse check without model or GitHub writes:

```bash
deepseek github action --event event.json --event-name issue_comment --dry-run
deepseek github action --event event.json --event-name issue_comment --mode patch --dry-run
deepseek github action --event event.json --event-name issue_comment --dry-run --require-mode fix,patch
```

In GitHub Actions, pass `--github-output` with `--dry-run` to append the resolved
`reference`, `number`, `mode`, and related fields to `$GITHUB_OUTPUT`. Combine it
with repeated or comma-separated `--require-mode` values to fail early when a
write workflow receives a review-only trigger.

For write workflows, resolve the branch to check out with:

```bash
deepseek github pr-head owner/repo#123 --repo-owner owner --github-output
```

The command calls `gh pr view --json headRepositoryOwner,headRefName`, prints a
small JSON summary, and appends `head_ref` / `head_owner` to `$GITHUB_OUTPUT`.
For `owner/repo#123` references it calls `gh pr view 123 --repo owner/repo` so
the resolver still works from a checked-out workflow repository. `--repo-owner`
refuses fork-owned PR branches before the workflow performs a write-capable
checkout.

For a local no-network smoke of the Action bridge:

```bash
deepseek github fixture-smoke --json
```

The smoke creates a temporary bare remote plus working repository, resolves a
review trigger, resolves a write trigger, verifies the fork-owned PR-head guard,
simulates a same-repository PR-head checkout, commits a fixture change, pushes it
back to the fixture remote branch, verifies the pushed head SHA, and exercises
`github action --background-task --task-no-run` against a temporary git repo to
prove task record/worktree creation. This is a local workflow gate; it does not
replace running the checked-in workflows in a real fixture repository.

The repository includes `.github/workflows/deepseek-code-review.yml` as a
disabled-by-default workflow example. Enable it by setting the repository
variable `DEEPSEEK_CODE_REVIEW_ENABLED=true` and the secret `DEEPSEEK_API_KEY`.
The workflow grants `pull-requests: write` / `issues: write` so `--post` can add
PR comments through the GitHub token.

The checked-in workflow pins `--mode review` as the safe default because local
`pr fix` / `pr patch` require a PR-head checkout before mutating files. For a
write-capable workflow, add a checkout step that checks out the PR head branch,
then run `deepseek github action --mode auto` or an explicit `--mode fix` /
`--mode patch`.

The repository also includes `.github/workflows/deepseek-code-write.yml` as a
disabled-by-default write example. Enable it with
`DEEPSEEK_CODE_WRITE_ENABLED=true`. It only reacts to `@deepseek fix` /
`@deepseek patch`, resolves the PR target with `--dry-run --github-output
--require-mode fix --require-mode patch`, resolves the PR head with
`deepseek github pr-head`, refuses fork-owned PR branches, checks out the
same-repository PR head, runs
`deepseek github action --mode auto`, and commits/pushes resulting workspace
changes back to the PR branch. Keep this workflow disabled unless you are
comfortable granting `contents: write` to the repository token.
