# Background Worktree Runner

Date: `2026-05-23`
Status: `implemented local runner + GitHub Action delegation slice`

## Goal

Close the local background-task part of the Claude Code / Codex app-cloud gap
without waiting for a hosted service. A user should be able to start a long
coding task in an isolated git worktree, let the parent CLI exit, then inspect
the task metadata, logs, resulting diff, and either merge or reject the result.

## Scope

This slice adds a repo-local `deepseek task` surface:

- `deepseek task start "<task>"`
- `deepseek task list`
- `deepseek task show <id>`
- `deepseek task stop <id>`
- `deepseek task diff <id>`
- `deepseek task merge <id>`
- `deepseek task reject <id>`
- `deepseek task fixture-smoke --json`
- `deepseek github action --background-task`

`task start` creates `.dscode/task-runner/worktrees/<id>` with
`git worktree add -b deepseek-task/<id>`, records metadata under
`.dscode/task-runner/records/<id>.json`, writes stdout/stderr logs under
`.dscode/task-runner/logs/`, and launches `deepseek exec --json` in that
worktree. `--no-run` creates only the worktree and record, which gives CI and
release checks a no-credential local gate. `task diff` renders the tracked patch
or stat plus untracked files. `task merge --check` validates the tracked patch
and untracked-file targets; `task merge` applies the patch and copies untracked
regular files back to the original repo, requiring a clean original worktree
unless `--allow-dirty` is explicit. `task reject` marks the record rejected and
removes only the managed task worktree unless `--keep-worktree` is supplied.

`deepseek github action --background-task` reuses the GitHub event parser and
mode resolver, but creates a background task instead of running
`deepseek pr review/fix/patch` inline. `--task-id` gives the task a stable id,
and `--task-no-run` creates only the worktree/record for local workflow smoke
tests or deferred execution.

## Non-Goals

- Hosted/cloud execution.
- Hosted merge queues or automatic commits/pushes.
- Replacing durable runtime thread tasks or the agents daemon.
- Satisfying model-backed dogfood gates without real online runs.

## Acceptance

- Parser routes `task start/list/show/stop/diff/merge/reject/fixture-smoke`
  into a first-class CLI command.
- Task ids are path-safe and cannot escape `.dscode/task-runner/records`.
- `task start --no-run` creates an isolated git worktree and durable record.
- `task list --json` and `task show --json` expose machine-readable metadata.
- `task stop` marks records stopped and terminates a live recorded process when
  the platform supports it.
- `task diff` exposes tracked and untracked task worktree changes.
- `task merge --check` validates patch application without mutating the original
  repo.
- `task merge` applies tracked changes and untracked regular files back to the
  original repo and marks the record `merged`.
- `task reject` marks the record `rejected` and removes the managed task
  worktree by default.
- `task fixture-smoke --json` proves the local no-model create, list, merge
  check, merge apply, and reject path in a temporary git repository.
- `github action --background-task --task-no-run` creates a task record and
  isolated worktree from a resolved PR event without credentials or network.

## Evidence

- `cargo test task_record_round_trips_json --lib -- --test-threads=1`
- `cargo test validate_task_id_rejects_path_escape --lib -- --test-threads=1`
- `cargo test cli_from_argv_routes_task_runner_subcommands --lib -- --test-threads=1`
- `cargo test cli_from_argv_parses_github_action_background_task_flags --lib -- --test-threads=1`
- `cargo test github_background_task_prompt_includes_action_context --lib -- --test-threads=1`
- `cargo run --quiet -- task fixture-smoke --json`
- `cargo run --quiet -- github fixture-smoke --json`
- `.github/workflows/ci.yml` runs `deepseek task fixture-smoke --json` against
  Linux/macOS/Windows debug binaries and `deepseek github fixture-smoke --json`
  against the same debug binaries.
- `.github/workflows/release.yml` runs `deepseek task fixture-smoke --json`
  and `deepseek github fixture-smoke --json` against each release-matrix binary
  before packaging.

## Remaining

- Real hosted GitHub workflow evidence still needs a disposable repository and
  online model-backed run.
