# DeepSeek-TUI Network Policy Parity

Date: 2026-05-13

## Gap

DeepSeek-TUI has a dedicated `network_policy` module for outbound host
decisions, including allow/deny/default behavior, deny-wins precedence, and
plaintext audit logging.
DeepSeekCode had read-only `web_run`, `web_search`, `fetch_url`, and `finance`
tools plus localhost/private-host blocking, but no user-configurable host
policy for normal external domains and no network decision audit trail.

## Scope

- Add a shared network-policy decision module.
- Add `network.default`, `network.allow`, and `network.deny` config fields.
- Add environment overrides with `DSCODE_NETWORK_DEFAULT`,
  `DSCODE_NETWORK_ALLOW`, and `DSCODE_NETWORK_DENY`.
- Add `network.audit` and `network.audit_path` config fields plus
  `DSCODE_NETWORK_AUDIT` / `DSCODE_NETWORK_AUDIT_PATH` overrides.
- Enforce the policy in the shared web fetch path used by `fetch_url`,
  `web_search`, `web_run`, `image_query`, and `finance`.
- Append best-effort plaintext audit lines for attempted web fetch decisions.
- Preserve existing behavior by defaulting to `allow`.
- Keep localhost/private-host blocking unchanged unless
  `DSCODE_ALLOW_LOCAL_FETCH=1` is set.
- Route `prompt` decisions through the existing AgentLoop/runtime/TUI
  permission request flow when tools execute through the registry.
- Document the remaining direct-tool fail-closed behavior.

## Acceptance

- `network.deny` blocks matching hosts before a fetch.
- `network.allow` allows matching hosts when the default is `deny` or `prompt`.
- Deny rules win over allow rules.
- Leading-dot rules match subdomains but not the apex domain.
- `network.default = "prompt"` creates a `kind = "network"` permission request
  for AgentLoop/runtime/TUI execution and marks only the approved tool
  invocation as network-approved.
- Direct web tool execution without the registry approval path fails closed with
  a clear approval-required message.
- `network.audit = true` appends host/tool/decision audit lines without making
  audit write failures fatal.
- Config and environment overrides are documented.
- Runtime docs and the DeepSeek-TUI parity plan mention the network policy.

## Implementation

- Added `src/core/network_policy.rs` with host normalization, exact and
  subdomain matching, default decisions, and deny-wins precedence.
- Added `NetworkConfig` to `AppConfig`.
- Parsed `network.default`, `network.allow`, `network.deny`, `network.audit`,
  and `network.audit_path` from `.dscode/config.toml`.
- Added comma-separated env overrides for the same fields.
- Wired `src/tools/web.rs` to load policy at the common fetch validation point.
- Wired prompt-host discovery into `src/tools/registry.rs` so `fetch_url`,
  `web_search`, `web_run`, `image_query`, and `finance` can emit network
  permission requests before execution.
- Added a hidden per-invocation approval marker that the registry injects after
  an approved `kind = "network"` request; web tools never expose this marker in
  their public schema.
- Added best-effort audit logging through the same validation point.
- Added default config rendering for the network policy fields.
- Updated README, runtime docs, and the DeepSeek-TUI parity plan.

## Verification

- `cargo test network_policy --lib` passed: 6 tests.
- `cargo test network_audit --lib` passed: 2 tests.
- `cargo test parse_config_overrides_network_policy_from_toml --lib` passed.
- `cargo test fetch_url_respects_network_policy_deny --lib` passed.
- `cargo test fetch_url_appends_network_audit_line --lib` passed.
- `cargo test permission_request_for_reports_network_prompt --lib` passed.
- `cargo test fetch_url_prompt_policy_requires_approval_unless_marked --lib`
  passed.
- `cargo test web_run_open_exposes_links_and_click_fetches_target --lib`
  passed, including cached-link prompt approval target discovery.
- `cargo fmt --check` passed.
- `git diff --check` passed.
- `cargo test -- --test-threads=1` passed: 1097 tests.
- `cargo package --allow-dirty` passed.

## Remaining Differences

- Direct web tool execution outside AgentLoop/registry approval remains
  fail-closed for `prompt` mode.
- Approved network prompts are per tool invocation; DeepSeekCode does not yet
  persist per-host "always allow" decisions back into config.
