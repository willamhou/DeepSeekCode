# DeepSeek-TUI FIM Edit Parity Spec

日期：2026-05-13

对比对象：`Hmbown/DeepSeek-TUI`，`main` HEAD `3382242`

## 背景

DeepSeek-TUI added a V4 Fill-in-the-Middle tool named `fim_edit`. Its upstream
implementation reads a workspace file, finds `prefix_anchor` and
`suffix_anchor`, sends the prefix and suffix to DeepSeek's `/beta/completions`
FIM endpoint, and writes the generated middle back into the file. DeepSeekCode
currently has exact `edit_file` replacement but no `fim_edit` compatibility
surface.

## 目标

- Add agent-visible `fim_edit`.
- Match the upstream input shape: `path`, `prefix_anchor`, `suffix_anchor`, and
  optional `max_tokens`.
- Read only safe workspace-relative paths and reject symlink path components.
- Find `prefix_anchor`, then find `suffix_anchor` after the prefix anchor.
- Generate the middle text through DeepSeek `/beta/completions` when an API key
  is configured.
- Support an optional `generated_text` test/offline override so the tool can be
  validated without live API access.
- Write `prefix + generated_text + suffix` back to the file and return
  structured metadata including generated length and byte boundaries.
- Expose `fim_edit` in the default registry, write approval surface, and model
  schema.

## 非目标

- This slice does not stream FIM output.
- This slice does not implement a dedicated FIM model router.
- This slice does not bypass existing write approval policy.

## 验收标准

1. `fim_edit` replaces only the middle between `prefix_anchor` and
   `suffix_anchor`.
2. Missing prefix or suffix anchors return actionable errors.
3. Unsafe paths and symlink path components are rejected.
4. Without `generated_text`, the tool calls a DeepSeek `/beta/completions`
   endpoint using configured model settings and API key env var.
5. `fim_edit` appears in the default registry.
6. `fim_edit` appears in model tool schemas.
7. `fim_edit` is classified as a write tool for permission requests.

## 实现结果

- Added `FimEditTool` in `src/tools/file_write.rs`.
- Matched the upstream input shape: `path`, `prefix_anchor`, `suffix_anchor`,
  and optional `max_tokens`.
- Reused the existing workspace-relative path checks and symlink-component
  refusal used by file write tools.
- Implemented anchor lookup in order: prefix anchor first, suffix anchor only
  after the prefix anchor.
- Added DeepSeek `/beta/completions` invocation through `curl` when no
  `generated_text` override is provided. The endpoint resolves to
  `<base_url>/beta/completions`, or `<base_url>/completions` when the configured
  base URL already ends in `/beta`.
- Added `generated_text` as an offline/test override to validate write behavior
  without live API credentials.
- Registered `fim_edit` in the default registry, model schema, permission
  request write classification, runtime docs, and parity plan.

## 验证

- `cargo test fim_edit`: passed, 3 tests.
- `cargo test parse_fim_completion_text`: passed.
- `cargo test build_tool_specs_include_file_write_tools`: passed.
- `cargo test permission_request_for_reports_write_shell_and_mcp_prompts`:
  passed.
- `cargo test default_registry_includes_read_only_git_history_tools`: passed.
- `cargo test`: passed, 993 tests.
- `cargo fmt --check`: passed.
- `git diff --check`: passed.
- `cargo package --allow-dirty`: passed; packaged 294 files and verified.
