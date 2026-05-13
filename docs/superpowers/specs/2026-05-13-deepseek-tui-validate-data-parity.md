# DeepSeek-TUI Validate Data Parity Spec

日期：2026-05-13

对比对象：`Hmbown/DeepSeek-TUI`，`main` HEAD `3382242`

## 背景

DeepSeek-TUI exposes `validate_data` for validating JSON/TOML from inline content
or workspace files. DeepSeekCode had a local JSON parser for runtime/model data,
but no model-visible structured data validation tool.

## 目标

- Add `validate_data` as a read-only agent tool.
- Accept either `path` or `content`, with `format = auto|json|toml`.
- Use strict JSON validation through the existing local parser.
- Provide a lightweight no-new-dependency TOML validator for common config shapes.
- Expose the tool through the default registry, model schemas, MCP, and ACP.

## 非目标

- This slice does not introduce a full TOML parser dependency.
- This slice does not validate against JSON Schema.
- This slice does not implement YAML validation.

## 验收标准

1. Default registry exposes `validate_data`.
2. OpenAI/Anthropic schemas include `path`, `content`, and `format`.
3. JSON inline content validates and reports top-level summary.
4. TOML file content validates in `auto` mode by extension.
5. Invalid auto content reports both JSON and TOML errors.
6. MCP/ACP tool listing and call dispatch include `validate_data`.

## 实现结果

- `src/tools/validate_data.rs` adds `ValidateDataTool`.
- `src/tools/registry.rs`, `src/model/deepseek.rs`, and
  `src/cli/commands/serve.rs` expose it to agents, schemas, MCP, and ACP.
- `docs/runtime.md` and the DeepSeek-TUI parity plan document the new tool.

## 验证

- `/home/willamhou/.cargo/bin/cargo fmt --check`
- `/home/willamhou/.cargo/bin/cargo test validate_data`
- `/home/willamhou/.cargo/bin/cargo test default_registry_includes_read_only_git_history_tools`
- `/home/willamhou/.cargo/bin/cargo test mcp_tools_list_includes_workspace_and_runtime_tools`
- `/home/willamhou/.cargo/bin/cargo test acp_session_tools_list_new_session_is_read_only`
- `/home/willamhou/.cargo/bin/cargo test`
- `git diff --check`
- `/home/willamhou/.cargo/bin/cargo package --allow-dirty`
