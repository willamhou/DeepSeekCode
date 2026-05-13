# DeepSeek-TUI RLM Python Chunk Helper Parity Spec

日期：2026-05-13

对比对象：`Hmbown/DeepSeek-TUI`，`main` HEAD `3382242`

## 背景

DeepSeek-TUI 的 RLM REPL 内置 `chunk_context(max_chars, overlap)` 和
`chunk_coverage(chunks)`。模型可以先把大输入按字符窗口完整切片，再用 coverage
检查是否漏掉区间。DeepSeekCode 当前 `rlm_python` / `rlm_python_session` 暴露
`context`，但缺少这两个 helper，模型需要手写切片逻辑，更容易遗漏边界或重复错误。

## 目标

- 给 `rlm_python` 和 `rlm_python_session` sandbox 注入 `chunk_context()`。
- 给 sandbox 注入 `chunk_coverage(chunks)`。
- helper 读取已有 `context` 变量，输出 JSON-serializable list/dict。
- 校验 `max_chars > 0` 且 `overlap < max_chars`。
- 覆盖 summary 返回 chunks 数、context_chars、covered_chars、gaps、complete。

## 非目标

- 不在 Python helper 内直接调用 LLM。
- 不改变 `state` 持久化格式。
- 不实现完整 RLM turn loop。

## 验收标准

1. `rlm_python` 能用 `chunk_context` 切分 `context`。
2. `chunk_coverage` 能报告完整覆盖。
3. `rlm_python_session` 同样可用这两个 helper。
4. 非法 chunk 参数在 sandbox 内返回清晰错误。

## 实现结果

- `RLM_PYTHON_SANDBOX` 注入 `chunk_context(max_chars=20000, overlap=0)`。
- `chunk_context` 返回 `index`、`start`、`end`、`text` 字段，覆盖当前
  `context` 字符串。
- `chunk_coverage(chunks)` 返回 `chunks`、`context_chars`、`covered_chars`、
  `gaps`、`complete`。
- helper 可用于 `rlm_python` 和 `rlm_python_session`，后者可把 coverage 写入
  persisted `state`。
- model schema、runtime 文档和 DeepSeek-TUI parity plan 已补充 chunk helper 说明。

## 验证

- `/home/willamhou/.cargo/bin/cargo fmt --check`
- `/home/willamhou/.cargo/bin/cargo test rlm_python_chunk_helpers`
- `/home/willamhou/.cargo/bin/cargo test rlm`
- `/home/willamhou/.cargo/bin/cargo test build_tool_specs_include_rlm`
- `git diff --check`
- `/home/willamhou/.cargo/bin/cargo test`
- `/home/willamhou/.cargo/bin/cargo package --allow-dirty`
