# DeepSeekCode 当前状态与后续路线

最后更新：2026-05-24

## 最终目标

DeepSeekCode 的目标是成为一个 DeepSeek-first 的 code agent CLI：用户在终端里运行
`deepseek` 后，可以像使用 Claude Code CLI、Codex CLI 或 DeepSeek-TUI 一样，完成真实
仓库里的读代码、改代码、跑命令、查看 diff、继续修复、恢复会话和发布前验证。

当前执行口径收敛到 Linux/macOS 本地 code agent CLI。只要用户能在 Linux/macOS 安装并
运行 `deepseek`，稳定进入 TUI/REPL，完成模型读写代码、shell 验证、diff review、
resume 和本地 runtime/shell-supervisor 工作流，就可以认为这个 milestone 成立。
Windows、hosted IDE 和 npm 发布属于更大的产品硬化目标；Homebrew 已是经过 smoke
验证的 Linux/macOS 分发路径。

## 当前判断

DeepSeekCode 已经可以实际用于 Linux/macOS dogfood 和仓库内代码任务。它有全屏 TUI、
line-oriented REPL、持久 runtime、权限工具、shell/PTY、本地服务 smoke、MCP/ACP、
background worktree task runner、GitHub Action bridge、model-backed demo 和真实 online
dogfood 证据。

但它还不是“可以公开宣称等同 Claude Code CLI / Codex CLI”的成熟产品。剩余差距主要是：

- npm registry 的发布凭据与公开安装验证；
- 更多真实外部 repo 样本；
- 持续维护精简的新用户文档、public beta 说明和故障排查路径；
- hosted IDE、真实安装后的 systemd/launchd service smoke，以及更广的 Windows 长尾验证。

## 已经成立的证据

- `deepseek` 裸入口、TUI entrypoint、task worktree runner、GitHub Action bridge、
  shell fixture、service smoke 和 multi-file fixture scaffold 已纳入 CI gate。
- PR #16 的完整 CI 已通过 Linux、macOS 和 Windows：
  https://github.com/willamhou/DeepSeekCode/actions/runs/26334525472
- PR #16 记录并验证了 online multi-file external fixture evidence：
  `.dscode/dogfood/external-fixture-python-invoice-multifile-verification.json`
  中 `post_validation_passed=true`、`release_evidence_ready=true`。
- PR #14 引入的 Linux/macOS CLI readiness gates 已在 hosted debug binary 上通过：
  https://github.com/willamhou/DeepSeekCode/actions/runs/26333425574
- online dogfood release gate 已达到 100+ model-backed run 口径；当前 live plan 曾显示
  `105` 条 online run、`99` 条 success，分类为 `write_validate 29/30`、
  `recovery 23/25`、`pr_workflow 47/50`。
- README 首屏已切换到真实交互式 2048 过程录屏：终端 SVG 展示 `deepseek chat`
  从空 repo 接收用户 prompt、写出 `2048.html`、完成 shell 校验并总结运行方式；配套
  GIF/MP4 展示同一次生成结果的浏览器试玩。旧 scripted 2048、TUI 和 edit/test SVG
  已降级为补充 demo/evidence。
- `v0.1.3` 已发布 GitHub Release binaries，并通过 Release Matrix：
  https://github.com/willamhou/DeepSeekCode/actions/runs/26351958964
- `v0.1.3` release assets 覆盖 Linux x64、Linux arm64、macOS x64、macOS arm64 和
  Windows x64；新增 Linux arm64 build 在 hosted `ubuntu-24.04-arm` 上完整通过。
- `v0.1.3` Release Smoke 已在干净 hosted Linux x64、Linux arm64、macOS x64 和
  macOS arm64 runner 上验证公开 release binary 下载、checksum、解压和 install smoke：
  https://github.com/willamhou/DeepSeekCode/actions/runs/26352088322
- `v0.1.3` GHCR image 已由 workflow 推送，公开 registry manifest 可读取，digest 为
  `sha256:f7f1574e100bd491cf2e8ddfa4aefccca5a957867b97199fd930f0e6b0af9fc9`。
- Homebrew tap 已发布到 `willamhou/homebrew-deepseekcode`，canonical tap 命令
  `brew tap willamhou/deepseekcode && brew install deepseek` 已通过 macOS x64/arm64
  Homebrew Smoke：
  https://github.com/willamhou/DeepSeekCode/actions/runs/26352180898
- `v0.1.3` npm packaging metadata、download-plan 和 publish-status 检查已就绪；
  npm registry 发布因 repository secrets 中缺少 `NPM_TOKEN` 被 tag workflow 明确跳过，
  当前 `npm view @deepseek-code/cli` 仍为 registry 404。
- PR #18 增加 `deepseek quickstart` / `deepseek onboarding` 首跑检查，并通过 CI：
  https://github.com/willamhou/DeepSeekCode/actions/runs/26335387193
- PR #19 增加 `deepseek update release-smoke`，用于发布二进制复验，并通过 CI：
  https://github.com/willamhou/DeepSeekCode/actions/runs/26348829744
- 外部 write-fixture 生成器已扩展为 Python、Rust、Node 三个 disposable repo 样本；
  CI 会 smoke scaffold。Node task-report 和 Rust order multi-file 样本也已记录
  online model-backed evidence，Python invoice 样本仍是 canonical release path。
- DeepSeek-native loop 的 repair/cache 证据已补齐：`deepseek dogfood
  repair-cache-evidence --json` 会生成
  `.dscode/dogfood/repair-cache-evidence.json`，记录 before/after runtime
  threads，并可用 `deepseek events replay`、`deepseek events diff` 和
  `deepseek stats --thread --require-prefix-stable` 验证 `tool_call_repair`、
  prompt-layer 事件和 cache hit/miss delta。Release Matrix packaging job 现在
  会固定运行这条确定性证据链，并上传 `deepseek-loop-evidence` JSON artifact。

## 当前能力概览

- 入口：`deepseek`、`deepseek chat`、`deepseek run`、`deepseek tui`、`deepseek exec`。
- TUI：Plan / Agent / YOLO 模式、approval modal、command palette、session/thread 视图、
  runtime-backed `/goal`、MCP 管理、setup/onboarding、provider/model picker。
- 首跑：`deepseek quickstart` 以只读方式展示 workspace config、API key env、model/base
  URL、TTY 状态、下一步命令和 starter tasks；`deepseek config provider
  [show|list|<name> [model]]`、`deepseek config model [show|list|<model>]` 和
  `deepseek config auth [ENV] --stdin` 已支持 provider/model 选择与安全 `.env` 写入；
  `--json` 可用于安装验证和自动化排障。
- REPL：raw-mode line editor、history、session list/load completion、SIGINT cancel、
  `/save`、`/load`、`/sessions`、custom slash commands。
- Runtime：`.dscode/runtime/` 下持久化 sessions、threads、turns、items、events、
  thread goals、tasks、usage、automations，并提供 HTTP/SSE runtime surface。
- 工具：文件读写/search、patch、diff、shell、background jobs、diagnostics、review、
  notes、memory、rollback、skills、subagents、MCP/ACP。
- Shell/PTY：Linux native PTY、bounded interactive attach、byte stream、raw proxy、
  Linux `pty_fd` handoff、shell-supervisor protocol bridge；Windows ConPTY/TCP path 已有
  CI smoke，但不是 Linux/macOS milestone blocker。
- 自动化：background worktree task runner，GitHub Action review/fix/patch bridge，
  disabled-by-default hosted review/write workflow examples。
- 发布：GitHub Release、GHCR、release package、Homebrew tap、npm package staging、
  Homebrew formula rendering、secret scan、publish-status readiness audit。

## 剩余工作

### Linux/macOS 本地 CLI milestone

这个限定目标的核心交互能力和 evidence gate 已经成立。下一步主要是样本厚度和文档维护：

1. 可选再增加 1-2 个真实外部 repo fixture，扩大 multi-file/多语言样本厚度。
2. 持续维护 README、install、release、current-status、public-beta 和 dogfood evidence
   文档；当前推荐首跑入口是 `deepseek quickstart`，README 只保留安装、试用、证据入口。

### 更大产品目标

1. 按 [DeepSeek-Native Agent Loop Design](./deepseek-native-loop.md) 推进
   cache-first prompt layers、tool-call repair、cost-aware model presets、
   read-only parallel dispatch 和 stats/replay surfaces。tool-call repair
   初版已落地：可修复可恢复的截断 JSON 参数、从显式 JSON-shaped 文本中找回已知工具调用，
   支持 `model.tool_schema_flattening = "auto"` 下的 schema flatten/re-nest，并在
   TUI runtime/`exec --json` 中留下 repair 证据；不可修复的 malformed tool-call parse
   failure 会转成下一步模型可见的 failed `model` observation，而不是直接硬失败；重复工具调用守卫已区分只读和写状态工具，
   prompt-layer diagnostics 与 `deepseek stats` MVP 也已接入 exec、TUI 和 runtime daemon
   turns，并可展示 per-layer token/hash trend 与 cache-stable hash-change totals，
   `deepseek stats --require-prefix-stable` 可作为 cache-stable prompt layer hash
   regression gate，runtime daemon compaction threshold/keep-tail 也已可配置；
   `model.preset = "auto" | "flash" | "pro"`、`deepseek config preset`、
   `run/exec --preset`、`--pro-next`、TUI `/pro`、`/pro off`、`/pro show` 和
   `model.session_budget_microusd` 的 80% warning / 100% refusal 初版也已落地，runtime
   session/thread records 会同步 `session_budget_microusd`，在 TUI/daemon 进程重启后用
   durable usage 恢复已用成本，`deepseek config budget raise <MICROUSD>`、`deepseek
   config budget +<MICROUSD>`、`deepseek config budget off` 和 TUI `model budget ...`
   会清晰处理 raise/disable runtime limit；auto escalation 已覆盖 repeated repair、
   malformed tool-call、tool-call storm、empty read/search、validation-after-edit 和
   repeated unproductive step signals，默认 live dogfood plan/report/evidence gate
   现在也要求 MCP dynamic/resource loop-surface 覆盖与至少 3 条 `mcp` live
   runs 的 gate，剩余工作是用真实 online runs 做 calibration；同回合 batch 中的本地
   read/search/git/project-map/data-validation 工具、常见 runtime query 工具，以及
   MCP inventory/prompt/resource 只读桥接工具现在会在无 hooks/permission/repeat 的情况下
   按连续 read-only chunk 并发，并保持结果顺序，tool result 会记录 `meta.parallel_*`
   telemetry，写入、shell、任意 `mcp_call` / dynamic MCP tool 和审批路径仍是串行
   barrier；`deepseek
   events replay <thread>` 和 `deepseek events diff <left> <right>` 初版也已接入
   runtime events/items/usage，可输出 text 或 JSON 证据；`deepseek dogfood
   repair-cache-evidence --json` 已补齐确定性的 before/after repair/cache 证据。
2. 配置 `NPM_TOKEN` 并发布 npm wrapper，验证 `npm install` 后裸 `deepseek` 入口。
3. 配置 `HOMEBREW_TAP_TOKEN`，让后续 tag workflow 自动更新 tap；当前 `v0.1.3` tap
   已手动发布并验证。
4. 在干净 Linux/macOS 机器上安装 systemd/launchd user services，记录
   `service-doctor --installed` 和 `service-smoke --installed` 证据。
5. 补真实 VS Code CLI runner 或 manual GUI fixture 证据。
6. 持续和 Claude Code CLI / Codex CLI / DeepSeek-TUI 做核心 loop 对照，只保留会影响真实用户使用的差距。

## 推荐公开表述

> DeepSeekCode is usable today for Linux/macOS dogfooding and repository work,
> with a full-screen TUI, REPL, durable runtime, permissioned tools, hosted
> Linux/macOS shell-supervisor smoke gates, release binaries including Linux
> arm64, clean hosted release-smoke evidence, a verified Homebrew tap, a 100-run
> online dogfood release gate, verified online multi-file external fixture
> evidence, real hosted GitHub workflow evidence, and committed real
> model-backed README interactive 2048 terminal and gameplay demo media. The
> supplemental scripted 2048, TUI, and edit/test demos remain available as
> evidence links. The
> remaining Linux/macOS CLI work is broader external sample depth and continuing
> documentation polish; hosted IDE, Windows/service proof, and npm publishing
> remain broader product-hardening work.
