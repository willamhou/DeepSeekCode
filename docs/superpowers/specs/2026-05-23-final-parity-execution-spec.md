# Final Parity Execution Spec

Date: `2026-05-23`
Status: `active`

## Goal

Drive DeepSeekCode toward the original goal: a DeepSeek-first code agent CLI
whose practical gap to DeepSeek-TUI, Claude Code CLI, Kimi-style coding CLI, and
Codex CLI is within about 5% for the daily coding loop.

The target is not another feature inventory. The target is evidence that a user
can install `deepseek`, open the TUI or REPL, ask for a real repository change,
let the model inspect and edit code, run validation, review the diff, recover
from failures, resume context, and use external integrations without hidden
manual glue.

## Current Evidence Snapshot

Local checks run during this execution pass:

- `cargo test --workspace --all-targets`: passed.
- `cargo fmt --check`: passed.
- `git diff --check`: passed.
- `cargo run --quiet -- benchmark --category mcp --out /tmp/deepseek-goal-mcp-benchmark.md`:
  `3/3` passed, filtered history/trend/live gates skipped as expected.
- `cargo run --quiet -- dogfood live-plan --limit 5 --json`: model transport is
  `offline`; model-backed live runs are `0`.
- `cargo run --quiet -- dogfood report --limit 20 --require-live-runs 1
  --require-live-category pr_workflow:1:90 --require-live-category recovery:1:90
  --require-live-category write_validate:1:90`: failed closed because all three
  required model-backed categories have `0` live runs.
- `cargo run --quiet -- update publish-status --strict --json`: failed closed
  with 5 not-ready/skipped checks: npm token, release assets, npm artifacts, live
  evidence verification, and Homebrew tap credentials.
- `node editors/vscode/test-extension-host.js`: skipped because no `VSCODE_BIN`,
  `code`, `code-insiders`, or `codium` CLI is available on this machine.

The repo already has strong local evidence for the CLI/TUI/runtime loop,
MCP/hooks/skills/subagents, background worktree runner, GitHub Action parser
fixture smoke, release packaging metadata, and deterministic README demo assets.

Live execution update from this pass:

- `dogfood live-run --api-key-file <outside-repo-key> --category write_validate
  --limit 1 --execute`: online, success.
- `dogfood live-run --api-key-file <outside-repo-key> --category recovery
  --limit 1 --execute`: online, success.
- First `pr_workflow` live run exposed a real stuck case: the model repeatedly
  called `project_map` instead of applying an explicit `replace X with Y in
  path` instruction after reading the target file.
- The runtime now adds a remote-mode direct-edit guardrail: once the target
  content has been read, it applies the explicit patch, runs the suggested test,
  and finishes after passing validation.
- Re-running the same `pr_workflow` case succeeded with `apply_patch` followed
  by `npm test`.
- A follow-up 5-case batch exposed that expected failure-readback cases should
  not be counted as release live success samples; live-plan now skips those
  cases. The retry case now succeeds by applying `a * b`, reading back the failed
  validation state, retrying with `a + b`, and passing `cargo test`.
- Current local live ledger is `20` online runs, `18` successes, and `2`
  historical stuck runs. The latest small gate passed at `live-runs=20`,
  overall success `90%`, `write_validate:5:90`, `recovery:10:90`, and
  `pr_workflow:5:80`. This is useful smoke evidence but does not satisfy the
  release gate below.

## Residual Gap Table

| Area | Current state | Gap to close | Gate |
|---|---|---|---|
| Core CLI/TUI coding loop | Usable; full tests and 82-case benchmark baseline are green in existing reports | Mostly evidence depth, not missing local primitives | Full test + default benchmark + recent no-stuck dogfood |
| Model-backed dogfood | Offline replay evidence exists; live plan reports `0` model-backed runs | Need 100 online runs; `write_validate`, `recovery`, `pr_workflow` each need 25 runs at >=90% success | `dogfood report --require-live-runs 100 --require-live-success-rate 90 --require-live-category write_validate:25:90 --require-live-category recovery:25:90 --require-live-category pr_workflow:25:90` |
| External write fixtures | Tooling and evidence JSON exist | Need 3-5 disposable real repo online write-fixture samples | `dogfood external-fixture ... --evidence-out` plus verifier |
| README real demo | Deterministic SVG exists; recorder/verifier exist | Need reviewed model-backed media artifact, not offline rehearsal | `record-model-backed-demo.sh`, verifier, rendered media committed |
| Windows Shell/PTY proof | Linux PTY fd/proxy path is strong; Windows ConPTY/TCP compile and workflow wiring exist | Need actual Windows runner evidence for ConPTY/TCP shell supervisor and fixture smoke | Windows CI/release job logs and artifact summary |
| Installed service proof | service-doctor/service-smoke local gates exist | Need clean-machine installed systemd/launchd smoke evidence | `agents service-smoke --installed ... --json` on real install |
| VS Code workbench | Native panel and headless fixture exist | Need extension-host run with real VS Code CLI and manual GUI fixture evidence | `VSCODE_BIN=... npm --prefix editors/vscode run test:extension-host` plus manual checklist |
| GitHub automation | Local event parser, write workflow, and fixture smoke exist | Need hosted fixture PR review and write workflow run evidence with online model | GitHub Actions run links/artifacts |
| Release channels | GitHub/GHCR/package metadata exist; publish-status is fail-closed | Need npm token, Homebrew tap token/repo, release dist/npm artifacts, live evidence verification artifact | `update publish-status --strict --dist ... --npm-dist ... --live-evidence-verification ...` |
| Public docs | README/current-status are good but long | Need final concise user path once evidence exists | README/current-status/release docs updated from evidence |

## Execution Order

1. Finish local correctness and commit hygiene.
   - Done in this pass: split local work into focused commits, fixed the shell
     log snapshot race, and verified the workspace.

2. Produce online model-backed evidence.
   - Run `deepseek dogfood live-run --api-key-file <outside-repo-key>
     --category write_validate --limit 5 --execute --evidence-out <path>`.
   - Repeat for `recovery` and `pr_workflow`.
   - Continue until the live evidence gate reaches 100 model-backed runs and all
     three categories meet 25 runs at >=90%.
   - Verify each batch with `deepseek dogfood live-evidence --file <path>
     --require-report-gate --out <verification-path>`.
   - This is blocked in this workspace until a valid DeepSeek key file is
     available outside the repo.

3. Capture real demo and external fixture evidence.
   - Run `docs/demo/record-model-backed-demo.sh` with a repo-external key file.
   - Verify the transcript and render the media.
   - Run at least 3 disposable real repositories through
     `deepseek dogfood external-fixture --workdir <repo> --benchmark-gate
     --evidence-out <path>`.
   - This is blocked on online model access and disposable repos.

4. Close IDE/GitHub hosted evidence gaps.
   - Run the VS Code extension-host smoke on a machine with `code` or `codium`.
   - Capture manual GUI evidence for diagnostic -> patch -> diff -> validation.
   - Run the review and write workflows in a fixture GitHub repository and record
     the resulting comments/commits.
   - This is blocked on VS Code CLI availability and hosted GitHub credentials or
     a fixture repository.

5. Close shell/service platform proof.
   - Preserve Linux PTY/fd/proxy evidence.
   - Collect Windows ConPTY/TCP shell fixture CI evidence.
   - Run installed service smoke on clean Linux/macOS machines.

6. Publish and update final public docs.
   - Configure `NPM_TOKEN` or `NODE_AUTH_TOKEN`.
   - Configure `HOMEBREW_TAP_REPOSITORY` and `HOMEBREW_TAP_TOKEN`.
   - Pass release `--dist`, npm `--npm-dist`, and live evidence verification to
     `deepseek update publish-status --strict --json`.
   - Update README/current-status with exact evidence links and remove outdated
     gap statements.

## Stop Conditions

Do not claim the 5% target while any of these are true:

- model-backed live dogfood remains below 100 runs;
- any of `write_validate`, `recovery`, or `pr_workflow` is below 25 online runs
  or below 90% success;
- VS Code and GitHub hosted evidence is only local/headless;
- npm/Homebrew publish checks remain credential-skipped;
- Windows shell-supervisor ConPTY/TCP evidence has not completed on a real
  Windows runner.

## Next Local Action

The next unblocked local action is to keep the repo green and preserve the
fail-closed gates while waiting for external evidence inputs. Once a DeepSeek
key file is available outside the repository, start with the `write_validate`
live-run batch because it directly proves the core inspect -> edit -> validate
loop.
