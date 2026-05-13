# DeepSeek-TUI ACP Parity Spec

日期：2026-05-12

对比对象：`Hmbown/DeepSeek-TUI`，`main` HEAD `3382242`

## 背景

DeepSeek-TUI 的 `docs/RUNTIME_API.md` 定义了 `deepseek serve --acp`：
一个面向 Zed/custom editor clients 的 Agent Client Protocol stdio adapter。
其首版范围很保守，只覆盖 JSON-RPC over newline-delimited stdio 的
baseline：`initialize`、`session/new`、`session/prompt`、`session/cancel`。

DeepSeekCode 在本轮前仍对 `serve --acp` 返回 not implemented，因此编辑器
端无法按 ACP 把它当作 agent server。

## 目标

落地最小 ACP stdio adapter，先满足 editor integration 的 baseline，而不把
现有 permissioned tool surface 暴露给 ACP。

## 非目标

本轮不做：

- ACP file-write / shell tools
- checkpoint replay
- loading existing durable sessions through ACP
- permissioned tool bridging
- streaming token-by-token updates

## 验收标准

1. `deepseek serve --acp` 不再报 not implemented。
2. 支持 newline-delimited JSON-RPC 2.0：
   - `initialize`
   - `session/new`
   - `session/prompt`
   - `session/cancel`
   - `shutdown`
3. `initialize` 返回 ACP protocol version、agent capabilities、agent info 和
  空 `authMethods`。
4. `session/new` 创建进程内 session，并支持 optional `cwd`。
5. `session/prompt` 从 string / text block / resource block / resource link
   提取 prompt text，通过当前 DeepSeek model client 生成回复，先发送
   `session/update`，再返回 `stopReason: "end_turn"`。
6. `serve --acp --workspace <path>` 从指定 workspace 解析 config 和默认
   session cwd。
7. 单元测试覆盖 CLI parsing、initialize、prompt block extraction、
   session/prompt update/result sequence。
8. release 文档包含 ACP stdio smoke。

## 实施结果

已落地：

- `src/cli/app.rs`
  - `ServeAction::Acp(ServeAcpArgs)`
  - `serve --acp --workspace <path>` parsing
- `src/cli/commands/serve.rs`
  - ACP stdio JSON-RPC loop
  - in-process ACP session registry
  - `initialize` / `session/new` / `session/prompt` / `session/cancel` /
    `shutdown`
  - `session/update` agent message chunk emission
  - no-tool prompt execution through `DeepSeekClient`
- `README.md`、`docs/runtime.md`、`docs/release.md`
  - 更新 ACP adapter 和 release smoke
- `docs/superpowers/plans/2026-05-10-deepseek-tui-parity.md`
  - Phase G2 标记 ACP baseline landed

## 剩余差距

- 当前 DeepSeekCode ACP baseline 与 DeepSeek-TUI `docs/RUNTIME_API.md` 记录的
  初版 ACP adapter 范围持平：同样是 newline-delimited JSON-RPC，
  覆盖 `initialize` / `session/new` / `session/prompt` / `session/cancel`，
  并以 single `session/update` chunk 结束当前 turn。
- DeepSeekCode ACP prompt 目前是 non-streaming single update；这不是相对
  DeepSeek-TUI 当前首版 baseline 的缺口，但完整 ACP editor 体验仍需要更细粒度
  streaming。
- ACP durable session list/load 已拆到并落地于
  `2026-05-12-deepseek-tui-acp-session-load-parity.md`。
- ACP 还不能做 checkpoint replay。
- ACP 还没有 permissioned tool bridge；这必须复用现有 approval model 后再开放。
