# MCP Fixture Smoke Slice

**Status:** local fixture smoke plus model-planning benchmark slice landed
**Parent plan:** `docs/superpowers/plans/2026-05-10-claude-codex-gap-closure-v2.md`

## Gap

Phase 12D requires fixture-backed MCP coverage for stdio, HTTP, SSE, remote tool
schema discovery, and agent-facing dynamic tool injection. The repo already had
MCP client/server implementations and unit tests, but there was no single local
command that exercised the end-to-end surface across all three transports.

## Implemented

- New CLI route:
  - `deepseek mcp fixture-smoke`
  - `--json` emits a machine-readable report
- The smoke creates a temporary fixture workspace and MCP config.
- The stdio leg uses the current DeepSeekCode binary through
  `serve --mcp --workspace <fixture>`.
- The HTTP and SSE legs use in-process local loopback MCP fixtures; no network
  outside `127.0.0.1` is required.
- The smoke verifies:
  - stdio tool discovery
  - HTTP tool discovery
  - SSE tool discovery
  - stdio `read_file` tool call
  - HTTP `echo` tool call
  - SSE `echo` tool call
  - stdio/HTTP/SSE prompt discovery and prompt get
  - stdio/HTTP/SSE resource discovery and resource read
  - stdio/HTTP/SSE resource template discovery
  - dynamic `mcp__server__tool` exposure through the default agent registry
  - cached input schemas for dynamic remote MCP tools
  - one broken stdio server does not hide or break healthy server discovery
  - generic `mcp_call` produces an MCP permission request
  - dynamic `mcp__server__tool` calls produce an MCP permission request
  - allowlisted generic `mcp_call` can execute
  - allowlisted dynamic MCP tools can execute
  - non-allowlisted generic and dynamic MCP calls are denied by policy

The JSON report now includes:

- `bad_server_isolated`
- `mcp_call_permission_ok`
- `dynamic_permission_ok`
- `mcp_call_allow_ok`
- `mcp_call_allowlist_deny_ok`
- `dynamic_allow_ok`
- `dynamic_allowlist_deny_ok`
- `stdio_prompt_ok`
- `http_prompt_ok`
- `sse_prompt_ok`
- `stdio_resource_ok`
- `http_resource_ok`
- `sse_resource_ok`
- `stdio_templates`
- `http_templates`
- `sse_templates`

The default benchmark manifest now also carries four MCP planning cases:

- `fixture-mcp-dynamic-readme` verifies the offline planner can call a dynamic
  `mcp__stdio-self__read_file` tool directly.
- `fixture-mcp-generic-call-readme` verifies the generic `mcp_call` path for
  `stdio-self/read_file`.
- `fixture-mcp-resource-workspace` verifies MCP resource discovery followed by
  `mcp_read_resource` against the fixture workspace resource.
- `fixture-mcp-allowlist-deny-recovery` verifies a denied `mcp_call` recovers by
  listing configured MCP tools instead of falling into repository search or
  generic replanning.

The benchmark runner can inject a per-case self MCP fixture by writing an
isolated `.dscode/mcp.json`, enabling dynamic tool exposure per case, and
setting a case-local MCP allowlist.

## Verification

- `cargo test cli::commands::mcp --lib`
- `cargo test cli::commands::benchmark --lib`
- `cargo test offline_planner_routes_dynamic_mcp_read_file --lib`
- `cargo test offline_planner_routes_generic_mcp_call --lib`
- `cargo test offline_planner_lists_mcp_tools_after_policy_deny_hint --lib`
- `cargo test parses_mcp_subcommands --lib`
- `cargo test cli::commands::help --lib`
- `cargo run --quiet -- mcp fixture-smoke --json`
  - Latest local JSON included `stdio_prompt_ok=true`,
    `http_prompt_ok=true`, `sse_prompt_ok=true`, `stdio_resource_ok=true`,
    `http_resource_ok=true`, `sse_resource_ok=true`, and template counts
    `3/1/1`.
- `DEEPSEEK_API_KEY_ENV=DEEPSEEK_API_KEY_OFFLINE cargo run --quiet -- benchmark
  --category mcp --out /tmp/deepseek-mcp-benchmark.md` passes the current
  default MCP slice at `4/4` deterministically through the offline planner.
- A historical `cargo run --quiet -- benchmark` refresh passed the prior
  `82/82` default manifest with the older `3/3` MCP slice; rerun a full default
  benchmark after the current 84-case manifest expansion when release evidence
  needs a fresh full baseline.

## Remaining

- No MCP-specific Phase 12D smoke gap is known after tool, schema,
  prompt/resource/template, bad-server isolation, and policy allow/deny
  coverage. Keep future work focused on broader Phase 12D hooks/skills/subagent
  gates unless external compatibility evidence raises a new MCP gap.
