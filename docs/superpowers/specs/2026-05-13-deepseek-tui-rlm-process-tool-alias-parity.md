# DeepSeek-TUI RLM Process Tool Alias Parity Spec

日期：2026-05-13

对比对象：`Hmbown/DeepSeek-TUI`，`main` HEAD `3382242`

## 背景

DeepSeek-TUI 的 changelog 和工具说明把完整长输入 RLM 入口称为 `rlm_process`，而当前
DeepSeekCode 只让 `rlm` / `rlm_query` / `llm_query` 兼容了 `task + file_path/content`
输入形状。模型或迁移提示如果直接调用 `rlm_process`，仍会找不到工具。

## 目标

- 新增模型可见 `rlm_process` tool 名称。
- `rlm_process` 复用现有 `RlmTool` process 输入路径。
- schema 公开 `task`、`file_path`、`content`、`steps`、`max_depth`。
- registry depth gate 与 `rlm` 保持一致：低于 subagent max depth 才暴露。
- 文档说明 `rlm_process` 目前是 bounded child-agent process adapter，不是完整 REPL
  loop。

## 非目标

- 本片不实现完整 DeepSeek-TUI long-lived REPL RLM turn loop。
- 不改变 `rlm` / `rlm_query` / `llm_query` 既有行为。

## 验收标准

1. 默认 registry 在 depth gate 内暴露 `rlm_process`。
2. depth limit 时隐藏 `rlm_process`。
3. OpenAI/Anthropic schema 包含 `rlm_process`。
4. `rlm_process` schema 包含 `task`、`file_path`、`content`、`max_depth`。

## 实现结果

- registry 在 RLM family depth gate 内注册 `RlmTool { tool_name:
  "rlm_process" }`。
- `rlm_process` 复用 `RlmTool` 的 `task + file_path/content` process 输入路径。
- model static tool schema 新增 `rlm_process`，包含 `task`、`file_path`、
  `content`、`steps`、`max_depth`。
- `docs/runtime.md` 和 DeepSeek-TUI parity plan 已说明该入口目前是 bounded
  child-agent adapter。

## 验证

- `/home/willamhou/.cargo/bin/cargo fmt --check`
- `/home/willamhou/.cargo/bin/cargo test rlm_process`
- `/home/willamhou/.cargo/bin/cargo test build_tool_specs_include_rlm`
- `/home/willamhou/.cargo/bin/cargo test default_registry_includes_dispatch_subagent_only_below_max_depth`
- `/home/willamhou/.cargo/bin/cargo test rlm`
- `git diff --check`
- `/home/willamhou/.cargo/bin/cargo test`
- `/home/willamhou/.cargo/bin/cargo package --allow-dirty`
