# Approval Fingerprint Parity

## Context

DeepSeek-TUI fixed generic approval-cache denials by including generic tool
input in approval fingerprints. DeepSeekCode does not use the same session
approval cache; runtime approvals are matched by durable `request_id`. The
nearest risk is external TUI/HTTP clients or future caches treating same-name
permission requests as interchangeable when their inputs differ.

## Spec

- Add a stable `fingerprint` field to each durable `permission_request`.
- Build the fingerprint from tool name, permission kind, target, and sorted
  input key/value fields.
- Keep fingerprints stable for exact repeated requests and distinct for
  same-tool requests with different targets or inputs.
- Parse and show the fingerprint in the TUI approval modal so operators can
  audit exact repeat approvals and denials.
- Document the field in the runtime contract.

## Verification

- `permission_request_fingerprint_tracks_exact_input`
- `append_permission_request_updates_thread_and_event_stream`
- `app_from_store_loads_permission_request_events`
- `cargo test permission_request --lib -- --test-threads=1`
- `cargo test app_from_store_loads_permission_request_events --lib`
- `cargo fmt --check`
- `cargo check`
- `cargo test --lib -- --test-threads=1`
- `git diff --check`
