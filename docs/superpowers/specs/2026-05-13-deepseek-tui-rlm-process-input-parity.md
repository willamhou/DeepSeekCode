# DeepSeek-TUI RLM Process Input Parity Spec

日期：2026-05-13

对比对象：`Hmbown/DeepSeek-TUI`，`main` HEAD `3382242`

## 背景

DeepSeek-TUI 当前的模型可见 RLM 入口是结构化 `rlm` tool：模型传 `task`，并通过
`file_path` 或 `content` 指向长输入。长输入被加载进 RLM Python REPL 的 `context`
变量，父模型只看到任务和元信息。DeepSeekCode 已有 `rlm(context, question)` 轻量入口，
但还不兼容 `task + file_path/content` 的调用形状；模型遇到大文件时仍需要把内容先塞进
`context` 参数。

## 目标

- 让现有 `rlm` / `rlm_query` / `llm_query` 兼容 `task + file_path/content` 输入。
- `file_path` 必须是 workspace 相对路径，禁止绝对路径和 `..` 逃逸。
- `content` 与 `file_path` 互斥，inline content 上限 200k chars。
- 读取文件后生成长输入处理任务，并通过现有 bounded child-agent 执行。
- 保留原来的 `context + question` RLM-lite 行为。
- schema 暴露 `task`、`file_path`、`content`、`max_depth`，让模型能按
  DeepSeek-TUI 形状调用。

## 非目标

- 本片不实现完整长驻 REPL RLM turn loop。
- 不允许 RLM process 输入读取 workspace 外部路径。
- 不改变 `rlm_batch` / `rlm_python_session` 语义。

## 验收标准

1. `rlm` / `rlm_query` / `llm_query` 仍支持 `context + question`。
2. `task + file_path` 会读取安全 workspace 相对文件并形成 RLM process 任务。
3. `task + content` 会形成 RLM process 任务，并拒绝超过 200k chars 的输入。
4. 同时传 `file_path` 与 `content`、缺少二者、绝对路径或 `..` 路径都会被拒绝。
5. model tool schema 包含 DeepSeek-TUI-style process fields。

## 实现结果

- `src/tools/rlm.rs` 的 `RlmTool` 支持双输入模式：
  - 没有 `task` / `file_path` / `content` 时，继续使用原 `context + question`。
  - 有 process 形状字段时，要求 `task` 且要求 `file_path` 或 `content` 二选一。
- `file_path` 只接受 workspace 相对路径，拒绝 absolute、parent/prefix component、
  directory，以及 canonicalize 后逃逸 workspace 的 symlink target。
- `content` 复用 200k char 上限，并拒绝空输入。
- process 模式会渲染包含目标、来源、字符/行数、覆盖要求和长输入的 child-agent 任务。
- OpenAI/Anthropic static tool schema 已给 `rlm` / `rlm_query` / `llm_query` 暴露
  `task`、`file_path`、`content`、`max_depth` 字段。
- `docs/runtime.md` 和 DeepSeek-TUI parity plan 已说明该兼容入口。

## 验证

- `/home/willamhou/.cargo/bin/cargo fmt --check`
- `/home/willamhou/.cargo/bin/cargo test rlm_process`
- `/home/willamhou/.cargo/bin/cargo test rlm`
- `/home/willamhou/.cargo/bin/cargo test build_tool_specs_include_rlm`
- `git diff --check`
- `/home/willamhou/.cargo/bin/cargo test`
- `/home/willamhou/.cargo/bin/cargo package --allow-dirty`
