# DeepSeek-TUI MCP HTTP Session Compatibility

## Context

DeepSeek-TUI commit `ece8055` tightened Streamable HTTP MCP interoperability by
preserving `Mcp-Session-Id` values across requests and by attempting a GET
preflight before the first POST. DeepSeekCode already replayed session ids
returned by initialize POST responses, but initialize itself could still fail
against servers that issue or require a session id before the first POST.

## Scope

- Add a best-effort GET preflight for HTTP MCP servers before initialize.
- Apply configured custom headers to the GET preflight.
- Capture `Mcp-Session-Id` from preflight or any POST response.
- Replay the current session id on initialize, initialized notifications,
  paginated list requests, and call/get/read requests.
- Preserve stdio and SSE behavior.

## Acceptance

1. HTTP MCP `tools/list` still works when the session id is returned by the
   initialize POST response.
2. HTTP MCP `tools/list` works when the session id is returned by GET preflight
   and must be replayed on initialize.
3. Custom MCP headers are sent on GET preflight as well as POST requests.
4. Focused MCP tests and formatting pass.

## Verification

- `/home/willamhou/.cargo/bin/cargo test list_remote_tools_summary_replays_http_preflight_session_id --lib`
- `/home/willamhou/.cargo/bin/cargo test supports_http_transport --lib`
- `/home/willamhou/.cargo/bin/cargo test validate_servers_summary_reports_mcp_surface_health --lib`
- `/home/willamhou/.cargo/bin/cargo test mcp --lib`
- `/home/willamhou/.cargo/bin/cargo fmt --check`
- `/home/willamhou/.cargo/bin/cargo check`
- `/home/willamhou/.cargo/bin/cargo test --lib -- --test-threads=1`
