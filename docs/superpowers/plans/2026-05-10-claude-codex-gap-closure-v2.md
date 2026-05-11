# Claude/Codex Gap Closure Plan V2

最后更新：`2026-05-10`
状态：`Phase 12A baseline refreshed; Phase 12B next`
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
- 已重建 `.dscode/dogfood/latest.md`：`39` runs，最新 6 条均为 `success`，没有新增 failed/stuck/manual
- 默认 benchmark manifest 已扩到 `67` cases，其中 `subagent` category 为 `20` cases
- 已连续跑 5 次完整离线 benchmark：`DEEPSEEK_API_KEY_ENV=DEEPSEEK_API_KEY_OFFLINE /home/willamhou/.cargo/bin/cargo run --offline -- benchmark`，最新 `67/67`
- Benchmark trend gate 已从 warmup 变为 `pass against 4 comparable runs`
- Benchmark live gate：`pass (runs 39)`

### Acceptance

- `cargo test --offline` 全绿：done
- `deepseek benchmark` 全绿：done
- `.dscode/benchmarks/latest.md` 不再显示 stale `48/49`：done；当前为 `67/67`
- `.dscode/dogfood/latest.md` 中新增 dogfood 不引入 failed/stuck/manual：done

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
- PR workflow benchmark 至少 25 条
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
- Hook benchmark covers prompt submit, pre tool, post tool, session start/stop
- Subagent benchmark >=20 cases
- No unbounded nested dispatch

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

Start Phase 12B:

1. Design the VS Code native agent workbench surface and event protocol.
2. Add a webview chat panel with streaming assistant output and tool trace state.
3. Inject active file, selection, diagnostics, and git diff summary into IDE-originated tasks.
4. Add patch review/apply/reject and validation command controls.
