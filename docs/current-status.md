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
- README 已提交真实 model-backed SVG，展示失败 Rust 测试、模型修改、通过 `cargo test`
  和最终 diff。
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

## 当前能力概览

- 入口：`deepseek`、`deepseek chat`、`deepseek run`、`deepseek tui`、`deepseek exec`。
- TUI：Plan / Agent / YOLO 模式、approval modal、command palette、session/thread 视图、
  MCP 管理、setup/onboarding、provider/model picker。
- 首跑：`deepseek quickstart` 以只读方式展示 workspace config、API key env、TTY 状态、
  下一步命令和 starter tasks；`--json` 可用于安装验证和自动化排障。
- REPL：raw-mode line editor、history、session list/load completion、SIGINT cancel、
  `/save`、`/load`、`/sessions`、custom slash commands。
- Runtime：`.dscode/runtime/` 下持久化 sessions、threads、turns、items、events、
  tasks、usage、automations，并提供 HTTP/SSE runtime surface。
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

1. 配置 `NPM_TOKEN` 并发布 npm wrapper，验证 `npm install` 后裸 `deepseek` 入口。
2. 配置 `HOMEBREW_TAP_TOKEN`，让后续 tag workflow 自动更新 tap；当前 `v0.1.3` tap
   已手动发布并验证。
3. 在干净 Linux/macOS 机器上安装 systemd/launchd user services，记录
   `service-doctor --installed` 和 `service-smoke --installed` 证据。
4. 补真实 VS Code CLI runner 或 manual GUI fixture 证据。
5. 持续和 Claude Code CLI / Codex CLI / DeepSeek-TUI 做核心 loop 对照，只保留会影响真实用户使用的差距。

## 推荐公开表述

> DeepSeekCode is usable today for Linux/macOS dogfooding and repository work,
> with a full-screen TUI, REPL, durable runtime, permissioned tools, hosted
> Linux/macOS shell-supervisor smoke gates, release binaries including Linux
> arm64, clean hosted release-smoke evidence, a verified Homebrew tap, a 100-run
> online dogfood release gate, verified online multi-file external fixture
> evidence, real hosted GitHub workflow evidence, and a committed real
> model-backed README demo SVG. The remaining Linux/macOS CLI work is broader
> external sample depth and continuing documentation polish; hosted IDE,
> Windows/service proof, npm publishing, and optional richer demo media remain
> broader product-hardening work.
