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
- `cargo run --quiet -- dogfood report --limit 100 --require-live-runs 100
  --require-live-success-rate 90 --require-live-category write_validate:25:90
  --require-live-category recovery:25:90 --require-live-category
  pr_workflow:25:90`: passed. `live-plan` reports `100` online runs, `94`
  successes, `write_validate 24/25`, `recovery 23/25`, and `pr_workflow 47/50`.
- `cargo run --quiet -- dogfood live-evidence --file
  .dscode/dogfood/live-evidence-final-total-pr-4.json --require-report-gate
  --out .dscode/dogfood/live-evidence-final-total-pr-4-release-verification.json
  --json`: passed release evidence validation with `report_gate_passed=true`.
- `cargo run --quiet -- update publish-status --strict --json
  --live-evidence-verification
  .dscode/dogfood/live-evidence-final-total-pr-4-release-verification.json`:
  failed closed with 4 not-ready/skipped checks: npm token, npm artifacts,
  release assets, and Homebrew tap credentials. Live evidence is now ready.
- `node editors/vscode/test-extension-host.js`: skipped because no `VSCODE_BIN`,
  `code`, `code-insiders`, or `codium` CLI is available on this machine.

The repo already has strong local evidence for the CLI/TUI/runtime loop,
MCP/hooks/skills/subagents, background worktree runner, GitHub Action parser
fixture smoke, release packaging metadata, and deterministic README demo assets.

Live execution update from this pass:

- The first `pr_workflow` live run exposed a real stuck case: the model repeatedly
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
- A later 5-case `pr_workflow` live batch exposed two more reproduce-and-fix
  stuck cases: the model gathered repo context but did not patch within budget.
  The guardrail now treats explicit direct-edit tasks as patchable once any
  successful repo-context observation exists (`read_file`, `list_files`,
  `list_dir`, `project_map`, or `search_text`), not only after reading the exact
  target content.
- Re-running that same 5-case `pr_workflow` batch succeeded `5/5`, including
  the previously stuck Python and Rust reproduce-and-fix cases.
- Expanding `write_validate` exposed a Python pytest retry case where the last
  readback was the test file rather than the edited file. The retry detector now
  treats `def test_` and `assert ` as test readbacks, so the guardrail can retry
  from the intentionally wrong `a * b` edit to `a + b` and pass pytest.
- Expanding `recovery` exposed an empty-search task that found no matches and
  inspected the repository layout, then kept listing files. The recovery
  guardrail now finishes once a no-match search and successful layout inspection
  are both observed.
- Current local live ledger satisfies the release gate: after the external
  fixture runs, `live-plan` reports `105` online runs and `99` successes;
  category counts are `write_validate 29/30`, `recovery 23/25`, and
  `pr_workflow 47/50`.

## Residual Gap Table

| Area | Current state | Gap to close | Gate |
|---|---|---|---|
| Core CLI/TUI coding loop | Usable; full tests and 82-case benchmark baseline are green in existing reports | Mostly evidence depth, not missing local primitives | Full test + default benchmark + recent no-stuck dogfood |
| Model-backed dogfood | Release live gate passed; current live plan reports `105` online runs and `99` successes, with categories `write_validate 29/30`, `recovery 23/25`, `pr_workflow 47/50` | Preserve verified evidence and keep the gate fail-closed in release status | `dogfood report --require-live-runs 100 --require-live-success-rate 90 --require-live-category write_validate:25:90 --require-live-category recovery:25:90 --require-live-category pr_workflow:25:90` |
| External write fixtures | `3` disposable real repo online write-fixture samples verified for Rust, Python, and JavaScript | Optionally expand to 5 samples and add a multi-file/dependency-backed fixture | `dogfood external-fixture ... --evidence-out` plus `dogfood external-evidence --require-successful-external-fixtures 1` |
| README real demo | Committed model-backed SVG exists at `docs/demo/deepseek-code-model-demo.svg`, generated from a verified online transcript | Optional polish: TUI/GIF/MP4 capture for launch pages | `record-model-backed-demo.sh`, verifier, rendered media committed |
| Windows Shell/PTY proof | Linux PTY fd/proxy path is strong; Windows ConPTY/TCP compile and workflow wiring exist | Need actual Windows runner evidence for ConPTY/TCP shell supervisor and fixture smoke | Windows CI/release job logs and artifact summary |
| Installed service proof | service-doctor/service-smoke local gates exist | Need clean-machine installed systemd/launchd smoke evidence | `agents service-smoke --installed ... --json` on real install |
| VS Code workbench | Native panel and headless fixture exist | Need extension-host run with real VS Code CLI and manual GUI fixture evidence | `VSCODE_BIN=... npm --prefix editors/vscode run test:extension-host` plus manual checklist |
| GitHub automation | Local event parser, write workflow, and fixture smoke exist | Need hosted fixture PR review and write workflow run evidence with online model | GitHub Actions run links/artifacts |
| Release channels | GitHub/GHCR/package metadata exist; publish-status accepts the live evidence artifact | Need npm token, Homebrew tap token/repo, release dist/npm artifacts | `update publish-status --strict --dist ... --npm-dist ... --live-evidence-verification ...` |
| Public docs | README/current-status are good but long | Need final concise user path once evidence exists | README/current-status/release docs updated from evidence |

## Execution Order

1. Finish local correctness and commit hygiene.
   - Done in this pass: split local work into focused commits, fixed the shell
     log snapshot race, and verified the workspace.

2. Produce online model-backed evidence.
   - Done in this pass: the release gate reached `100` online runs and all three
     required categories are at or above 25 runs and 90% success.
   - Done in this pass: `.dscode/dogfood/live-evidence-final-total-pr-4-release-verification.json`
     verifies online readiness, appended model-backed rows, matching ledger
     fingerprint, and `report_gate_passed=true`.
   - Continue to keep model keys outside the repository and rotate any key that
     was exposed in chat or terminal output.

3. Capture real demo and external fixture evidence.
   - Done in this pass: `docs/demo/record-model-backed-demo.sh` captured a real
     online disposable Rust crate loop: failing `cargo test`, `deepseek exec`,
     one-line patch, passing `cargo test`, and final diff.
   - Done in this pass: `docs/demo/verify-model-backed-demo.js` accepted the
     transcript and `docs/demo/render-model-backed-demo-svg.js` rendered
     `docs/demo/deepseek-code-model-demo.svg`.
   - Done in this pass: README English, Chinese, and Japanese pages now embed
     the model-backed SVG below the deterministic TUI demo.
   - Done in this pass: Rust, Python, and JavaScript disposable repos under
     `/tmp/deepseek-external-fixtures/` each produced online external fixture
     evidence and passed verifier output:
     `.dscode/dogfood/external-fixture-rust-add-v3-verification.json`,
     `.dscode/dogfood/external-fixture-python-add-verification.json`, and
     `.dscode/dogfood/external-fixture-js-add-verification.json`.
   - Done in this pass: external fixture evidence records now include
     `model_backed`, so verifier ledger matching works for online rows.
   - Remaining external evidence work is reviewed demo capture and optional
     richer external fixtures, not the minimum three-sample disposable repo gate.

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

Cleared in this pass:

- model-backed live dogfood is now at `100` online runs with `94%` success;
- `write_validate`, `recovery`, and `pr_workflow` are each above 25 online runs
  and above 90% success.

Do not claim the 5% target while any of these remaining conditions are true:

- VS Code and GitHub hosted evidence is only local/headless;
- npm/Homebrew publish checks remain credential-skipped;
- Windows shell-supervisor ConPTY/TCP evidence has not completed on a real
  Windows runner.

## Next Local Action

The next unblocked local action is to keep the repo green and preserve the
fail-closed gates while collecting the remaining external evidence: hosted
GitHub workflow runs, VS Code CLI evidence, Windows ConPTY/TCP CI evidence,
optional richer external fixtures/demo media, and release-channel publish
artifacts.
