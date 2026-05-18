# DeepSeek-TUI Approval Grouping Parity

## Source

Comparison source: `Hmbown/DeepSeek-TUI` refreshed at
`/tmp/deepseek-tui-compare-20260514`, `origin/main` after `eeccf7d`.

Relevant upstream fix: `81bc2da fix(tui): revert v0.8.38 /model picker rework
and restore approval grouping`.

## Gap

DeepSeekCode already had durable runtime permission requests and TUI approval
modals, but every approval was one-shot. DeepSeek-TUI restored an
`approve for session` path that uses two different keys:

- exact argument fingerprints for denials;
- lossy/grouping fingerprints for session approvals.

Without that split, users either approve repetitive safe command variants one
by one, or a denial/approval risks being scoped too broadly.

## Implemented Behavior

- Runtime permission requests now include both `fingerprint` and
  `grouping_fingerprint`.
- Runtime permission responses now record `scope`, defaulting to `once` and
  allowing `session` and `cached`.
- The TUI approval modal keeps `y` / Enter as approve-once and adds `a` for
  approve-for-session.
- Runtime agent approvals reuse prior `scope=session` approvals by grouping
  fingerprint, and reuse denials only when the exact fingerprint matches.
- Session-scoped approval reuse appends a `permission_response` event with
  `scope=cached`, preserving durable audit evidence.

## Grouping Rules

- Shell approvals group by command family, e.g. `cargo build` covers
  `cargo build --release` but not `cargo test`.
- Patch approvals group by affected file paths.
- Write approvals group by tool plus path-like argument.
- Network approvals group by host.
- Unknown tools fall back to a stable hash of tool, permission kind, and input.

## Remaining Difference

This slice does not change the `/model` picker behavior from the same upstream
commit. DeepSeekCode already keeps model/provider picker data local and
non-blocking in the TUI, while the `/models` command path can continue to expose
provider-derived model lists.

## Validation

- Runtime tests cover grouped shell approvals, exact denials, and response
  scope persistence.
- TUI tests cover the new `a` approve-for-session action.
- Resolver tests cover grouped session approval reuse and exact denial scoping.

