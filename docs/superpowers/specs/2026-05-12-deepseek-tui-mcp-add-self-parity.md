# DeepSeek-TUI MCP Add-Self Parity Spec

日期：2026-05-12

对比对象：`Hmbown/DeepSeek-TUI`，`main` HEAD `3382242`

## 背景

上一轮已让 `deepseek serve --mcp` 运行 read-only MCP stdio server，但用户
仍需要手写 MCP config 才能把 DeepSeekCode 注册给其他 MCP clients。DeepSeek-TUI
提供 `deepseek-tui mcp add-self`，会解析当前 binary 路径并生成运行
`serve --mcp` 的 stdio server entry。

## 目标

补齐 self-registration helper，让用户一条命令把当前 DeepSeekCode binary
注册为 MCP stdio server。

## 验收标准

1. `deepseek mcp add-self` 默认写入用户级 MCP config。
2. 默认 server name 为 `deepseek`，可用 `--name <name>` 覆盖。
3. 可用 `--workspace <path>` 生成 `serve --mcp --workspace <path>` entry。
4. 可用 `--project` 写入当前项目级 `.dscode/mcp.json`，方便 release smoke
   和仓库局部配置。
5. 已存在同名 server 时拒绝覆盖，避免破坏用户已有 MCP 配置。
6. `deepseek serve --mcp --workspace <path>` 能从指定 workspace 启动。
7. 单元测试覆盖 CLI 解析、config 写入、duplicate 拒绝、`servers` legacy
   key 保留。

## 实施结果

已落地：

- `src/cli/app.rs`
  - `McpAction::AddSelf`
  - `McpConfigScope::{User, Project}`
  - `serve --mcp --workspace <path>` 解析
- `src/cli/commands/mcp.rs`
  - 解析当前 executable path
  - 写入 user/project MCP config
  - 兼容 `mcpServers` 和 legacy `servers` key
  - duplicate server name 防护
- `src/cli/commands/serve.rs`
  - MCP stdio server 可在启动前切换 workspace
- `README.md`、`docs/install.md`、`docs/runtime.md`、`docs/release.md`
  - 更新 self-registration 文档和 release smoke

## 后续差距

- ACP stdio adapter 已在后续 slice 落地最小 baseline，但 session loading、
  checkpoint replay 和 permissioned tool bridging 仍未实现。
- MCP server mode 仍只暴露 read-only tools，side-effectful tools 需要先设计
  durable approval bridge。
- MCP prompt/resource serving 已有 read-only baseline；side-effectful server
  tools 仍需要 durable approval bridge。
