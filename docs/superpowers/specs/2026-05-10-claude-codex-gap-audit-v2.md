# Claude/Codex Gap Audit V2

最后更新：`2026-05-10`
状态：`active Phase 12A baseline`

## 结论

以 `2026-05-10` 的公开官方资料和当前仓库证据为基线，`DeepseekCode` 已经接近一个强本地 CLI code agent 原型，但还没有接近 Claude Code / Codex 的完整产品面。

当前我给出的差距估计：

- 相对“本地 CLI 编码代理核心闭环”：约 `8% - 12%`，但完成声明仍被 100+ live dogfood 证据阻塞
- 相对“Claude Code / Codex 完整产品体验”：约 `32% - 36%`
- 只有执行本 spec 后面的 Phase 12A-12E，才可以把完整产品面的残余差距压到约 `8% - 10%`

这个评分不是模型质量的绝对 benchmark，而是产品能力面、可靠性证据、集成深度、用户可用性和自动化成熟度的加权估计。

## 官方基线

本轮只使用官方或一手文档作为对照基线：

- Claude Code IDE integration: `https://code.claude.com/docs/en/ide-integrations`
- Claude Code GitHub Actions: `https://code.claude.com/docs/en/github-actions`
- Claude Code MCP: `https://code.claude.com/docs/en/mcp`
- Claude Code hooks: `https://code.claude.com/docs/en/hooks`
- Claude Code subagents: `https://code.claude.com/docs/en/sub-agents`
- Claude Code slash commands / skills: `https://code.claude.com/docs/en/slash-commands`
- OpenAI Codex overview: `https://developers.openai.com/codex`
- OpenAI Codex CLI features: `https://developers.openai.com/codex/cli/features`
- OpenAI Codex IDE extension: `https://developers.openai.com/codex/ide`
- OpenAI Codex app: `https://developers.openai.com/codex/app`
- OpenAI Codex AGENTS.md: `https://developers.openai.com/codex/guides/agents-md`
- OpenAI Codex MCP: `https://developers.openai.com/codex/mcp`
- OpenAI Codex hooks: `https://developers.openai.com/codex/hooks`
- OpenAI Codex subagents: `https://developers.openai.com/codex/subagents`
- OpenAI Codex GitHub Action: `https://developers.openai.com/codex/github-action`
- OpenAI Codex Help Center overview: `https://help.openai.com/en/articles/11369540-using-codex-with-your-chatgpt-plan`

OpenAI docs MCP was not available in this Codex session. I attempted `codex mcp add openaiDeveloperDocs --url https://developers.openai.com/mcp`; the sandboxed write failed, and the escalated install request was aborted by the user, so this audit falls back to official OpenAI web pages only.

## 当前仓库证据

当前工作树已有未提交改动，不能把 `HEAD` 当成唯一证据：

- Modified: `.dscode/benchmarks.example.txt`
- Modified: `.dscode/benchmarks.txt`
- Modified: `src/core/loop_runtime.rs`
- Modified: `src/model/deepseek.rs`

已核实能力：

- CLI 主入口和子命令：`src/cli/app.rs` 支持 `chat/repl/interactive`、`run`、`exec`、`agents`、`benchmark`、`dogfood`、`diff`、`resume`、`config`、`doctor`、`smoke`、`pr`、`mcp`、`completion`、`update`、`version`
- 本地工具闭环：`src/tools/*` 覆盖 list/read/search/patch/shell/diff/todo/MCP/subagent
- Workspace instructions：`src/core/instructions.rs` 支持用户级 `AGENTS.md` 和项目级 `AGENTS.override.md` / `AGENTS.md` / `CLAUDE.md` / `.claude/CLAUDE.md`
- REPL slash/custom commands：`docs/repl.md` 记录了 `/save`、`/load`、`/todos`、`/cost`、自定义 `.dscode/commands/*.md`
- Hooks：`docs/repl.md` 与 `src/core/hooks.rs` 支持 session、prompt、tool、permission、subagent 和 `pre_compact` 事件
- MCP：`src/cli/commands/mcp.rs` 支持 project/user config、stdio、HTTP、legacy SSE 的 `tools/list`、`tools/call`、`prompts/list`、`prompts/get`，并有 dynamic tool schema、agent bridge 与 approval/allowlist
- Subagent：`src/tools/dispatch_subagent.rs` 有 bounded child loop、parallel dispatch、child summary、thread artifacts、child files、next action
- VS Code：`editors/vscode/extension.js` 是 terminal-backed extension，含 status bar、Explorer view、webview panel、selection context、diagnostics summary、active diff
- PR/CI local workflow：`src/cli/commands/pr.rs` 与 `src/integrations/github.rs` 通过 `gh` 集成本地 PR review/fix/patch
- Benchmark manifest：`.dscode/benchmarks.txt` 当前 67 条，其中 `subagent` category 为 20 条
- Dogfood ledger：`.dscode/dogfood/latest.md` 当前 39 runs，总成功率 79.5%，新增 Phase 12A replay 均为 success；`recovery` 和 `write_validate` 仍有历史低成功率

当前需要谨慎解释的证据：

- `.dscode/benchmarks/latest.md` 已刷新为 `67/67`，`plan-product-readiness` 和 20 条 subagent baseline 均通过
- 当前未提交代码已经增加 product readiness planning heuristic、manifest case 和单测
- 已运行验证：
  - `/home/willamhou/.cargo/bin/cargo test --offline product_readiness`，`1 passed`
  - `/home/willamhou/.cargo/bin/cargo test --offline`，`611 passed`
  - `DEEPSEEK_API_KEY_ENV=DEEPSEEK_API_KEY_OFFLINE /home/willamhou/.cargo/bin/cargo run --offline -- benchmark`，`67/67`
  - benchmark live gate：`pass (runs=39)`
  - benchmark trend gate：`pass against 4 comparable runs`
  - Phase 12A dogfood replay：6 条新增记录，均为 offline-fallback success
  - live DeepSeek endpoint in this sandbox：`curl: (6) Could not resolve host: api.deepseek.com`，未写入 ledger

因此，本 audit 把当前 benchmark 状态记为：`current working tree is green on unit tests and default offline benchmark; trend gate and live gate pass, but true remote dogfood depth remains insufficient`.

## Gap 评分

权重总分 100。分数表示当前 `DeepseekCode` 相对 Claude Code / Codex 完整产品面的接近程度。

| 维度 | 权重 | 当前分 | 证据 | 主要缺口 |
|---|---:|---:|---|---|
| 本地 CLI coding loop | 20 | 17 | tools、agent loop、patch、shell、diff、resume、REPL、exec JSONL 已有 | 在线模型稳定性和复杂任务收敛仍弱 |
| 质量门禁和 dogfood | 12 | 9 | benchmark 67/67，trend/live gate pass，dogfood ledger 39 runs | dogfood 成功率仍受历史失败拖累，真实外部样本薄 |
| PR / CI 本地 workflow | 10 | 7 | `deepseek pr review/fix/patch`、16 条 pr_workflow case | 没有 GitHub Action / @mention 自动化，真实 CI 样本少 |
| Memory / commands / hooks / MCP | 15 | 13 | AGENTS/CLAUDE、custom commands、expanded hooks、MCP stdio/HTTP/SSE/schema/prompts | hosted/plugin UX 和更厚 fixture 仍不足 |
| Subagent orchestration | 8 | 8 | bounded dispatch、parallel dispatch、thread artifacts、20 条 subagent baseline | 仍需要 live multi-agent dogfood 厚度 |
| IDE / editor experience | 12 | 4 | VS Code terminal-backed sidebar/panel/diff/diagnostics | 非原生 chat、非流式 patch UI、无自动 selection/diagnostics sync、无 JetBrains |
| App / cloud / background tasks | 12 | 1 | 无 `.github`、无 desktop/web app、无 cloud worker | Codex app/cloud threads/worktrees、Claude/Codex GitHub automation 缺失 |
| Packaging / security / admin | 7 | 5 | install docs、version、config init、approvals | release artifacts、sandbox hardening、enterprise controls 不足 |
| Multimodal / browser / computer/design workflows | 4 | 2 | `exec --image` 对 vision-capable transports 可发 native payload，MCP 可接 Figma/browser 类工具 | 无 computer use、browser automation 产品入口 |
| **总计** | **100** | **66** |  | **完整产品面 gap 约 34%** |

## 为什么不是 <10%

`DeepseekCode` 的强项已经覆盖 CLI local agent 的核心环节，但 Claude Code / Codex 的领先点不只是“能读改跑”：

1. IDE 是原生工作台，而不是 terminal-backed launcher
2. GitHub 自动化是产品面的一等能力，而不是本地 `gh` wrapper
3. Codex 有 app / cloud threads / worktree / background tasks 这一类持久任务面
4. Claude/Codex 的 hooks、subagents、skills、MCP 都有更完整的配置、权限和 UI
5. 官方产品已有更厚的真实工作流路径，当前 repo 的 dogfood 只有 39 runs，且其中新增 Phase 12A 样本为 offline-fallback replay；true live recovery/write_validate/pr_workflow 厚度仍不足

## Phase 12 目标状态

把完整产品面 residual gap 压到 `<10%` 的最低可接受状态：

- `deepseek benchmark` 当前 manifest 全绿，并且趋势门禁和 live gate 均通过
- dogfood 至少 `100` runs，`pr_workflow` / `recovery` / `write_validate` 每类至少 `25` runs，分类成功率均 `>=90%`，最近 20 条无 stuck/manual
- VS Code 从 terminal-backed launcher 升级为可用 agent workbench：
  - 原生 chat panel
  - streaming tool trace
  - selection/diagnostics 自动同步
  - inline diff review/apply/reject
  - task resume
- GitHub automation 有可运行的 `deepseek-action` 或等效 workflow：
  - PR review comment
  - issue/PR `@deepseek` trigger
  - CI log ingestion
  - optional patch commit / PR creation
- MCP 成为一等 agent tool surface：
  - schema 注入
  - per-tool approval policy
  - stdio/HTTP/SSE fixtures
  - failure isolation
- Subagent 支持显式并行任务：
  - disjoint write-set guidance
  - child patch conflict detection
  - parent readback gate
  - benchmark baseline 至少 20 条
- Packaging/release 可以让新用户在干净机器上安装、初始化、跑 doctor/smoke/benchmark
- 对 cloud/app 差距至少有 local background worktree runner 或 GitHub Action runner 兜底，不要求第一轮做完整商业云服务

## 非目标

Phase 12 不追求：

- 多模型通用平台化
- 完整云多租户服务
- 与 Codex app 一样的桌面产品完成度
- 完整 JetBrains 插件
- 大规模 AST 重构引擎

这些可以进入 Phase 13+。Phase 12 的判断标准是：相对 Claude Code / Codex 的主要 daily coding path，不再有阻塞级产品面缺口。

## 迭代后 residual gap 预估

| 轮次 | 交付后分数 | Residual gap | 说明 |
|---|---:|---:|---|
| 当前 | 59 | 41% | 强 CLI 原型，但 IDE/cloud/automation/reliability 仍明显薄 |
| Phase 12A: quality baseline | 66 | 34% | 先修绿当前 benchmark 和 dogfood 口径 |
| Phase 12B: IDE workbench | 75 | 25% | 日常编码入口接近 Claude/Codex IDE 面 |
| Phase 12C: GitHub automation | 82 | 18% | 补齐 PR/issue/CI 自动化主线 |
| Phase 12D: MCP/hooks/subagent hardening | 88 | 12% | 扩展面和多 agent 语义接近 |
| Phase 12E: background worktree runner + distribution | 91-92 | 8-9% | 用本地/CI background runner 兜住 Codex app/cloud 差距 |

只有 Phase 12E 完成且验收通过后，才能说差距小于 10%。
