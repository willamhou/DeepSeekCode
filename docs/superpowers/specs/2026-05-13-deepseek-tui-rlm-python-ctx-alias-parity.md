# DeepSeek-TUI RLM Python ctx Alias Parity Spec

日期：2026-05-13

对比对象：`Hmbown/DeepSeek-TUI`，`main` HEAD `3382242`

## 背景

DeepSeek-TUI 的 RLM REPL 把长输入暴露为 `context`，同时提供短别名 `ctx`。其系统
prompt、示例和 reference-aligned snippets 都可能使用 `ctx`。DeepSeekCode 当前
`rlm_python` sandbox 只暴露 `context`。

## 目标

- 在 `rlm_python` 和 `rlm_python_session` sandbox 中暴露 `ctx`。
- `ctx` 与 `context` 初始值完全一致。
- `SHOW_VARS()` 不把 `ctx` 当作用户变量报告。
- schema 和文档说明支持 `ctx` alias。

## 非目标

- 不让修改 `ctx` 回写 `context`；它只是输入别名。
- 不改变 `chunk_context` 的来源，它继续读取原始 `context` payload。

## 验收标准

1. `rlm_python` 中 `ctx == context`。
2. `rlm_python_session` 中也可使用 `ctx`。
3. `SHOW_VARS()` 不包含 helper alias `ctx`。

## 实现结果

- `RLM_PYTHON_SANDBOX` 在环境中加入 `ctx: _context_value`。
- `_helper_names` 包含 `ctx`，所以 `SHOW_VARS()` 不会把它当作用户变量。
- `rlm_python` 和 `rlm_python_session` 共享同一个 sandbox，因此两者都可使用 `ctx`。
- model schema、runtime 文档和 DeepSeek-TUI parity plan 已补充 `ctx` alias。

## 验证

- `/home/willamhou/.cargo/bin/cargo test rlm_python_ctx_alias`
- `/home/willamhou/.cargo/bin/cargo test rlm_python`
- `/home/willamhou/.cargo/bin/cargo fmt`
- `/home/willamhou/.cargo/bin/cargo fmt --check`
- `/home/willamhou/.cargo/bin/cargo test rlm`
- `git diff --check`
- `/home/willamhou/.cargo/bin/cargo test`
- `/home/willamhou/.cargo/bin/cargo package --allow-dirty`
