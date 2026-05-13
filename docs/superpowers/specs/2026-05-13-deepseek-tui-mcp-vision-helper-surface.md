# DeepSeek-TUI MCP Vision Helper Surface

Date: 2026-05-13

Status: completed

## Gap

`image_analyze` was already agent-visible for DeepSeek-TUI compatibility, but
MCP/ACP clients could not call it. Exposing it by default would be unsafe
because it can spend model tokens and make network calls to an
OpenAI-compatible vision endpoint.

## Spec

1. Keep `image_analyze` hidden from default MCP/ACP sessions.
2. Expose `image_analyze` only when trusted side effects are enabled or when a
   durable runtime approval thread is bound.
3. In durable approval mode, emit `permission_request kind=mcp` and wait for the
   matching response before invoking the vision API.
4. Reuse the existing `ImageAnalyzeTool` implementation and its workspace path,
   extension, API-key, and response parsing validation.
5. Add focused tests for default hiding/rejection, side-effect validation, and
   durable denial without reading an image or calling the network.

## Verification

- `/home/willamhou/.cargo/bin/cargo test mcp_tools_call_rejects_image_analyze_until_side_effects_enabled --lib`
- `/home/willamhou/.cargo/bin/cargo test mcp_tools_call_rejects_image_analyze_after_runtime_denial --lib`
- `/home/willamhou/.cargo/bin/cargo test mcp_tools_list_includes_run_shell_when_side_effects_enabled --lib`
- `/home/willamhou/.cargo/bin/cargo test mcp_tools_list_includes_write_tools_only_with_durable_approvals --lib`
- `/home/willamhou/.cargo/bin/cargo test mcp_ --lib`
- `/home/willamhou/.cargo/bin/cargo test acp_ --lib`
- `/home/willamhou/.cargo/bin/cargo fmt --check`
- `git diff --check`

## Implementation

- MCP `tools/list` advertises `image_analyze` only in trusted side-effect or
  durable approval modes.
- MCP `tools/call image_analyze` rejects default calls, routes durable calls
  through `permission_request kind=mcp`, and then delegates to `ImageAnalyzeTool`.
- ACP inherits the same gated visibility and call behavior through its
  session-scoped MCP adapter.
