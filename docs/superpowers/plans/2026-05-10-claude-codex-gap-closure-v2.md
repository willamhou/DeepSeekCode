# Claude/Codex Gap Closure Plan V2

最后更新：`2026-05-23`
状态：`Phase 12B underway`
关联 spec：`docs/superpowers/specs/2026-05-10-claude-codex-gap-audit-v2.md`

## 目标

把 `DeepseekCode` 相对 Claude Code / Codex 完整产品面的 residual gap 从当前约 `41%` 压到 `<10%`。

本计划按 5 轮推进。每轮都必须更新：

- gap audit 分数
- benchmark manifest 和 report
- dogfood ledger/report
- 该轮 spec/checklist
- 真实失败样本和后续修复项

## Completion Gate

只有同时满足下面条件，才能宣称 `<10%`：

1. `deepseek benchmark` 对当前默认 manifest 全绿
2. benchmark trend gate pass 或有明确 comparable reset 说明
3. dogfood live gate pass
4. dogfood 至少 100 runs，且 `pr_workflow` / `recovery` / `write_validate` 每类至少 25 runs
5. 上述三类 dogfood success rate 均 `>=90%`
6. 最近 20 条 dogfood 无 stuck/manual
7. VS Code agent workbench 可完成 selection -> diagnose -> patch -> diff review -> validate 的闭环
8. GitHub Action / workflow 可在 fixture repo 上完成 PR review comment 和 issue/PR trigger
9. MCP stdio/HTTP/SSE 均有 fixture-backed tool discovery/call/schema 注入验证
10. Subagent baseline 至少 20 条，覆盖 parallel read、parallel review、disjoint edits、child blocker、parent merge-back
11. 干净机器 install smoke：install -> config init -> doctor -> benchmark sample -> VS Code package syntax check

## Phase 12A - Fix Quality Baseline

目标 residual gap：`41% -> 34%`

### Deliverables

- 修复当前 `plan-product-readiness` benchmark 红点
- 重新跑完整 benchmark，生成新的 `.dscode/benchmarks/latest.md`
- 把 product-readiness planning case 保留在 `.dscode/benchmarks.example.txt`
- 增加至少 6 条 live dogfood replay，覆盖：
  - product gap planning
  - product readiness planning
  - recovery after failed validation
  - PR retry validate
- 更新 `docs/roadmap.md` 当前状态

### Implementation Notes

- 当前未提交代码已经在 `src/model/deepseek.rs` 增加 `task_looks_like_product_readiness`
- 当前未提交代码已经在 `src/core/loop_runtime.rs` 扩大 explicit planning trigger
- 已跑 targeted unit test：`/home/willamhou/.cargo/bin/cargo test --offline product_readiness`
- 已跑完整 unit test：`/home/willamhou/.cargo/bin/cargo test --offline`，`611 passed`
- 已追加 6 条 Phase 12A dogfood replay，覆盖 product gap planning、product readiness planning、failed-validation retry、Rust/JS/Python PR retry validate
- 当前 sandbox 无法解析 `api.deepseek.com`，这些 replay 使用 `DEEPSEEK_API_KEY_ENV=DEEPSEEK_API_KEY_OFFLINE` 走 offline fallback；真实在线 dogfood 仍是后续硬门槛
- 当前本机 dogfood ledger snapshot 为 `20` runs，`19` success、`1` historical failed、`0` stuck、`0` manual；`pr_workflow` offline replay 为 `14/14` success，`recovery` offline replay 为 `3/3`，`write_validate` 为 `2/3` success。真实在线 dogfood 仍是后续硬门槛
- 默认 benchmark manifest 当前为 `84` cases，其中 `subagent` category 为 `20` cases，`pr_workflow` category 为 `26` cases，`mcp` category 为 `4` cases
- `deepseek benchmark` 现在支持 `--category <name>` 与可重复 `--case <name>` targeted selection。Filtered benchmark run 只写 report，不推进 benchmark history，也不强制 full trend/live gate。当前 `cargo run --quiet -- benchmark --category subagent --out /tmp/deepseek-subagent-benchmark.md` 实测 `20/20`，trend/live gate 均为 filtered selection skip。
- `deepseek chat` / `repl` / `interactive` 现在在真实 TTY 中使用内置 raw-mode line editor，支持 Up/Down 历史、草稿恢复、左右移动、Home/End、Backspace/Delete、Ctrl+A/E/U/K/W、Tab slash/session completion、`/sessions [prefix]`、空行 Ctrl+D、prompt Ctrl+C 和运行中 turn 的 cooperative Ctrl+C cancellation，关闭了早期 REPL spec 中的 readline/history、session listing/load completion 与基础中断候选缺口。
- 默认 benchmark manifest 已扩到 `84` cases。最近完整离线 benchmark 的历史证据是扩展前 `82/82`；当前 84-case manifest 新增了 MCP resource loop-surface case，发布前需要刷新完整默认 benchmark。
- Benchmark trend gate：`skipped (need at least 3 prior comparable runs, found 2)`，因为 82-case history 当时仍在 comparable warmup
- Benchmark live gate：`pass against previous dogfood snapshot (runs 5 -> 20)`

### Acceptance

- `cargo test --offline` 全绿：done
- `deepseek benchmark` 全绿：历史 `82/82` done；当前 `84` case manifest 需要在发布前刷新完整 baseline
- `.dscode/benchmarks/latest.md` 不再显示 stale `48/49`：done；最新完整报告已刷新到 `.dscode/benchmarks/latest.md`
- `.dscode/dogfood/latest.md` 中新增 dogfood 不引入新的 failed/stuck/manual：done；current ledger snapshot still has `1` historical failed record, but the new `pr_workflow` and `recovery` replay batches are clean

## Phase 12B - Native VS Code Workbench

目标 residual gap：`34% -> 25%`

### Deliverables

- 把 VS Code extension 从 terminal launcher 升级为 agent workbench
- 新增原生 chat webview：
  - prompt input
  - streaming assistant output
  - tool trace list
  - cancel/stop
  - resume latest session
- IDE context 自动注入：
  - active file
  - selection
  - visible diagnostics
  - current git diff summary
- Patch review：
  - show proposed patch
  - apply/reject
  - open VS Code diff
  - run validation command
- Extension test harness：
  - `node --check editors/vscode/extension.js`
  - minimal command registry smoke
  - documented manual extension-host checklist

### Acceptance

- 从 VS Code 完成一个 fixture task：diagnostic -> run agent -> patch -> diff -> validation
- 不依赖用户手动复制 diagnostics
- 用户能在 IDE 内看见 patch 和 tool trace，而不只是 terminal 输出

## Phase 12C - GitHub Automation

目标 residual gap：`25% -> 18%`

### Deliverables

- 新增 `.github/workflows/deepseek-code-review.yml` 示例
- 新增 `deepseek github action` 使用文档，或独立 `action.yml` 初版
- 支持：
  - pull_request review
  - issue_comment / pull_request_review_comment trigger
  - `@deepseek` 默认触发词
  - CI log tail -> task prompt
  - optional patch branch/commit
- 新增 fixture repo workflow test plan
- 扩 `pr_workflow` benchmark：
  - PR review comment
  - issue to patch
  - CI log with lint failure
  - CI log with test failure
  - second-round review feedback

### Acceptance

- 在测试 repo 中，GitHub Action 能读取 PR diff 并发布 review comment
- `deepseek pr fix` 与 action 路径共享核心 prompt/context builder
- PR workflow benchmark 至少 25 条：done；默认 manifest 已有 `25` 条 `pr_workflow`
- PR workflow planner/action/comment-plan 离线 benchmark：done；当前非环境 PR 红点已清零
- dogfood `pr_workflow` 至少 25 runs，success rate `>=90%`

## Phase 12D - Extension Surface Hardening

目标 residual gap：`18% -> 12%`

### Deliverables

- MCP:
  - dynamic tool schema 注入到 agent tool definitions
  - stdio/HTTP/SSE fixtures
  - per-tool permission summary in prompt and logs
  - failure isolation: bad MCP server cannot break registry
- Hooks:
  - add `session_start`
  - add `stop`
  - add `subagent_start`
  - add `subagent_stop`
  - structured decision output for pre-tool hooks
- Skills/custom commands:
  - unify command and skill discovery docs
  - add skill metadata validation command
  - add examples for PR review, release, security-lite
- Subagents:
  - explicit parallel subagent request parser
  - disjoint write-set nudge
  - parent readback required for child-edited files
  - conflict/blocker summary

### Acceptance

- MCP benchmark covers stdio, HTTP, SSE, schema, approval allow/deny
  - Current: `mcp fixture-smoke` covers stdio/HTTP/SSE discovery/call/schema
    injection, prompt/resource/template discovery and reads, bad-server
    isolation, plus approval allow/deny; default benchmark adds dynamic MCP,
    generic `mcp_call`, and allowlist-deny recovery planning cases.
- Hook benchmark covers prompt submit, pre tool, post tool, session start/stop
  - Current: `hooks fixture-smoke` runs a local no-network agent-loop fixture
    that records `session_start`, `user_prompt_submit`, `pre_tool_use`,
    `post_tool_use`, and `session_stop`, verifies structured
    allow/add_context propagation, and proves a real `list_files` tool call
    completed.
- Skills/custom commands have discoverability docs and metadata validation
  - Current: `skills list` and `skills validate` scan the same bundled/user
    skill directories as runtime loading, report overrides and metadata
    warnings, and `skills validate --strict --json --dir skills` passes the
    bundled 16 skills with zero warnings/errors. Docs now include PR review,
    release-check, and security-lite skill examples.
- Subagent benchmark >=20 cases
  - Current: default benchmark manifest has 20 `subagent` category cases.
    `dispatch_subagents` now accepts per-child `write_scope` / `write_set`,
    reports per-child files/write scopes, aggregate readback next action,
    blocked child count, and write-scope conflicts. The parent planner consumes
    both single and parallel subagent summaries for mandatory readback.
    `agents subagent-fixture-smoke --json` locally verifies parser,
    disjoint write scopes, readback metadata, blocker summaries, conflict
    summaries, and artifact shape. `benchmark --category subagent` now verifies
    the 20-case subagent slice independently and currently passes `20/20` with
    history/trend/live gate writes skipped for the filtered run.
- No unbounded nested dispatch
  - Current: tool registry exposes dispatch tools only while child depth is
    below the bounded `MAX_SUBAGENT_DEPTH`.

## Phase 12E - Background Worktree Runner And Distribution

目标 residual gap：`12% -> 8-9%`

### Deliverables

- Local background worktree runner:
  - `deepseek task start`
  - `deepseek task list`
  - `deepseek task show`
  - `deepseek task stop`
  - isolated git worktree per task
  - log/session path per task
- Optional GitHub Action runner can delegate long tasks into isolated worktree
- Distribution:
  - release build script
  - install smoke on clean temp home
  - rollback docs
  - shell completion install docs
  - VS Code extension package build docs
- Security/admin:
  - sandbox mode docs
  - approval mode matrix
  - MCP trust model
  - hook trust model

Current first slice:

- `deepseek task start/list/show/stop/diff/merge/reject` is now a first-class
  CLI surface.
- `task start` creates an isolated `.dscode/task-runner/worktrees/<id>` git
  worktree, default branch `deepseek-task/<id>`, durable JSON metadata under
  `.dscode/task-runner/records/`, stdout/stderr logs under
  `.dscode/task-runner/logs/`, and launches `deepseek exec --json` there.
- `task start --no-run` creates only the worktree and record, giving release
  checks a no-credential path.
- `deepseek task fixture-smoke --json` creates a temporary git repo and proves
  the no-model worktree/record/list, merge check, merge apply, and reject paths
  locally.
- `task merge` requires a clean original worktree by default, applies the task
  patch plus untracked regular files back to the original repo, and marks the
  record `merged`; `task reject` marks the record `rejected` and removes only
  the managed task worktree unless `--keep-worktree` is explicit.
- CI runs the task fixture smoke against Linux/macOS/Windows debug binaries,
  and the Release Matrix runs it against each release binary before packaging.
- `deepseek github action --background-task` now converts a resolved GitHub PR
  event into the same local task-runner record/worktree instead of executing
  `pr review/fix/patch` inline; `--task-id` gives workflow-stable ids and
  `--task-no-run` supports local no-credential gates.

### Acceptance

- Long task can continue after parent CLI exits
- User can inspect and merge/reject background worktree diff
- Clean-machine smoke passes
- Release checklist includes benchmark, dogfood, action smoke, VS Code smoke

## Prompt-To-Artifact Checklist

| User requirement | Artifact/evidence |
|---|---|
| 看 repo 和 Claude Code / Codex 的差距 | `docs/superpowers/specs/2026-05-10-claude-codex-gap-audit-v2.md` gap table and official-source baseline |
| 根据 gap 设计 spec | `docs/superpowers/specs/2026-05-10-claude-codex-gap-audit-v2.md` Phase 12 target state and acceptance requirements |
| 根据 gap 设计 plan | This file, Phase 12A-12E |
| 重复 1-2 直到差距 <10% | Iteration table in spec plus this plan's phase residual gap targets |
| 不只凭感觉 | Current repo evidence, benchmark/dogfood evidence, targeted/full test output, and explicit trend/live gate results |

## Immediate Next Action

Phase 12B is now underway. The VS Code native agent panel slice landed in
`docs/superpowers/specs/2026-05-22-vscode-native-agent-panel.md`: the sidebar
webview runs `deepseek exec --json` directly, renders assistant/tool JSONL
events, supports cancel, injects active editor diagnostics and Git diff
context, resumes the latest exec session, provides active-file and workspace
changed-file review/accept/revert controls, and runs workspace validation
commands with captured output. The next slice added a generated patch artifact
queue: assistant/tool unified diffs are tracked separately from already-written
Git changes, pending artifacts can be opened/applied/rejected, single-file
artifacts open as VS Code diffs, and `Apply` verifies with `git apply --check`
before mutating the workspace. An extension-host smoke harness now creates a
temporary workspace with a mocked DeepSeekCode binary and drives the panel
provider inside a VS Code extension runner when `VSCODE_BIN`/`code` is
available. A headless panel fixture now covers diagnostic context injection,
assistant-generated patch capture, single-file diff opening, checked `git apply`,
workspace change refresh, and validation command success in a temporary Git repo.
Continue Phase 12B with the remaining evidence work:

1. Run the extension-host smoke on a machine with VS Code CLI available and
   record the output.
2. Capture manual GUI fixture evidence for diagnostic -> patch -> diff ->
   validation.

Phase 12C has also started. The first GitHub automation slice landed in
`docs/superpowers/specs/2026-05-22-github-action-bridge.md`: `deepseek github
action` parses GitHub event payloads into PR references, enforces `@deepseek`
triggers for comment/review events by default, supports `--dry-run`, and
delegates runs to the existing `deepseek pr review/fix/patch` implementations
through `--mode auto|review|fix|patch`. `--require-mode` and `--github-output`
now make the write workflow's target resolution a tested CLI behavior instead
of an embedded JSON-parsing script, and `deepseek github pr-head` moves PR head
owner/ref resolution plus fork-owned branch refusal into the same tested CLI
surface. `@deepseek fix` and `@deepseek patch` can now route to the existing
CI-log repair and patch workflows without a second implementation. The
repository includes a disabled-by-default
`.github/workflows/deepseek-code-review.yml` example guarded by
`DEEPSEEK_CODE_REVIEW_ENABLED=true`; it pins `--mode review` as the safe default
for summary comments. A second disabled-by-default write workflow,
`.github/workflows/deepseek-code-write.yml`, is guarded by
`DEEPSEEK_CODE_WRITE_ENABLED=true`, reacts only to `@deepseek fix` / `@deepseek
patch`, resolves target step outputs through `deepseek github action --dry-run
--github-output --require-mode fix --require-mode patch`, resolves and guards
the PR head through `deepseek github pr-head`, checks out the same-repository PR
head, and commits/pushes resulting changes after running `--mode auto`.
`deepseek github fixture-smoke` now provides a local no-network gate for review
target routing, write-mode routing, PR-head/fork guard behavior, a temporary
Git remote checkout/commit/push verification, and background task delegation via
`github action --background-task --task-no-run`. Continue Phase 12C with:

1. Run the workflow in a fixture repository and capture a real PR review
   comment.
2. Run the write workflow in a fixture repository and capture a PR-head
   checkout plus commit/push evidence for `--mode fix` / `--mode patch`.
3. Collect online `pr_workflow` dogfood evidence for the new action-labeled
   cases. Offline replay evidence is now available; hosted/model-backed
   evidence is still required.

Phase 12D has started with
`docs/superpowers/specs/2026-05-23-mcp-fixture-smoke.md`. `deepseek mcp
fixture-smoke --json` now creates a temporary MCP config, exercises the current
binary as a stdio MCP server, starts local loopback HTTP and SSE MCP fixtures,
verifies tool discovery and tool calls across all three transports, and proves
dynamic `mcp__server__tool` registry exposure plus input schema caching. The
same fixture now also injects a failing `broken-stdio` server and proves healthy
server discovery still succeeds, then verifies generic `mcp_call` and dynamic
`mcp__server__tool` permission request, allowlisted allow, and allowlist deny
paths. The benchmark runner now supports per-case self MCP fixtures, and the
default manifest adds four MCP planning cases for dynamic remote tool calls,
generic `mcp_call`, resource discovery/readback, and allowlist-deny recovery via
`mcp_list_tools`; the targeted MCP manifest previously passed `3/3` from `/tmp`
before the resource case was added, and the current 84-case default manifest
needs a fresh full benchmark refresh before release. The same fixture smoke now
also verifies stdio/HTTP/SSE prompts, resources, and resource templates in one
command.

Hooks now have `docs/superpowers/specs/2026-05-23-hooks-fixture-smoke.md`.
`deepseek hooks fixture-smoke --json` creates a temporary hook root and
workspace, installs structured allow/add_context recorder scripts, runs the
real agent loop with a deterministic fixture model client, triggers `list_files`,
and verifies `session_start`, `user_prompt_submit`, `pre_tool_use`,
`post_tool_use`, and `session_stop` payloads plus hook context propagation. The
latest local CLI smoke reports all lifecycle booleans true.

Skills/custom commands now have
`docs/superpowers/specs/2026-05-23-skills-custom-command-validation.md`.
`deepseek skills list` and `deepseek skills validate` expose the runtime skill
directory order as a user-runnable CLI, validate core TOML metadata and
allowed tool names, report same-name overrides, and support `--strict` for CI
gates. The latest local smoke `deepseek skills validate --strict --json --dir
skills` passes the bundled 16 skills with `error_count=0` and
`warning_count=0`.

Subagents now have
`docs/superpowers/specs/2026-05-23-subagent-fixture-smoke.md`.
`dispatch_subagents` accepts `write_scope` / `write_set`, exposes aggregate
parallel readback metadata, reports blocked child count and write-scope
conflicts, and persists the write scope into thread artifacts. The parent
offline planner now consumes both `dispatch_subagent` and `dispatch_subagents`
summaries for child-file readback. The latest local smoke `deepseek agents
subagent-fixture-smoke --json` reports all booleans true and `child_count=2`.
The targeted subagent benchmark now passes `20/20`, the historical full default
benchmark refresh passed `82/82` before the current 84-case manifest expansion,
and offline dogfood replay now covers the local release gate slices at
`runs=20`. Continue Phase 12D with online model-backed dogfood, refreshed full
benchmark evidence, and external compatibility evidence.
