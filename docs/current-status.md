# DeepSeekCode 当前状态与后续路线

最后更新：2026-05-23

## 最终目标

DeepSeekCode 的目标是成为一个 DeepSeek-first 的 code agent CLI：用户在终端里运行
`deepseek` 后，可以像使用 Claude Code CLI、Codex CLI 或 DeepSeek-TUI 一样，完成真实
仓库里的读代码、改代码、跑命令、查看 diff、继续修复、恢复会话和发布前验证。

当前执行口径收敛到 Linux/macOS 本地 code agent CLI。只要用户能在 Linux/macOS 安装并
运行 `deepseek`，稳定进入 TUI/REPL，完成模型读写代码、shell 验证、diff review、
resume 和本地 runtime/shell-supervisor 工作流，就可以认为这个 milestone 成立。
Windows、hosted IDE 和 npm 发布属于更大的产品硬化目标；Homebrew 是 macOS 分发打磨，
但不是核心交互能力 blocker。

## 当前判断

DeepSeekCode 已经可以实际用于 Linux/macOS dogfood 和仓库内代码任务。它有全屏 TUI、
line-oriented REPL、持久 runtime、权限工具、shell/PTY、本地服务 smoke、MCP/ACP、
background worktree task runner、GitHub Action bridge、model-backed demo 和真实 online
dogfood 证据。

但它还不是“可以公开宣称等同 Claude Code CLI / Codex CLI”的成熟产品。剩余差距主要是：

- release-binary 级别的下一轮 release matrix smoke 证据；
- Homebrew tap 和 npm registry 的发布凭据与公开安装验证；
- 更多真实外部 repo 样本；
- 更精简的新用户文档和故障排查路径；
- hosted IDE、真实安装后的 systemd/launchd service smoke、以及更广的 Windows 长尾验证。

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
- `v0.1.1` 已有 GitHub Release binaries、GHCR image、npm/Homebrew packaging metadata、
  release matrix、download-plan 和 publish-status 检查。

## 当前能力概览

- 入口：`deepseek`、`deepseek chat`、`deepseek run`、`deepseek tui`、`deepseek exec`。
- TUI：Plan / Agent / YOLO 模式、approval modal、command palette、session/thread 视图、
  MCP 管理、setup/onboarding、provider/model picker。
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
- 发布：GitHub Release、GHCR、release package、npm package staging、Homebrew formula
  rendering、secret scan、publish-status readiness audit。

## 剩余工作

### Linux/macOS 本地 CLI milestone

这个限定目标的核心交互能力和 evidence gate 已经成立。下一步主要是 release hardening：

1. 等下一次 release matrix 产出 release-binary 级别的 Linux/macOS shell/runtime smoke 证据。
2. 配置 Homebrew tap 凭据，完成 tap 发布和公开安装验证。
3. 可选再增加 1-2 个真实外部 repo fixture，扩大 multi-file/多语言样本厚度。
4. 继续压缩 README、install、release、current-status，让新用户能快速安装、配置、试用、排障。

### 更大产品目标

1. 配置 `NPM_TOKEN` 并发布 npm wrapper，验证 `npm install` 后裸 `deepseek` 入口。
2. 在干净 Linux/macOS 机器上安装 systemd/launchd user services，记录
   `service-doctor --installed` 和 `service-smoke --installed` 证据。
3. 补真实 VS Code CLI runner 或 manual GUI fixture 证据。
4. 持续和 Claude Code CLI / Codex CLI / DeepSeek-TUI 做核心 loop 对照，只保留会影响真实用户使用的差距。

## 推荐公开表述

> DeepSeekCode is usable today for Linux/macOS dogfooding and repository work,
> with a full-screen TUI, REPL, durable runtime, permissioned tools, hosted
> Linux/macOS shell-supervisor smoke gates, release binaries, a 100-run online
> dogfood release gate, verified online multi-file external fixture evidence,
> real hosted GitHub workflow evidence, and a committed real model-backed README
> demo SVG. The remaining Linux/macOS CLI work is Homebrew publishing,
> next-release binary smoke evidence, broader external sample depth, and
> documentation polish; hosted IDE, Windows/service proof, npm publishing, and
> optional richer demo media remain broader product-hardening work.
