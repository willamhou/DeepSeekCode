# DeepSeek-TUI Runtime Global SSE Parity Spec

Date: 2026-05-13

Source comparison: `Hmbown/DeepSeek-TUI`, `main` HEAD `3382242`

## Gap

HTTP-runtime TUI mode could subscribe only to thread event streams that were
known at startup. Cross-process writes to those threads were low latency, but a
new remote thread required the slower fallback refresh before the foreground TUI
could discover it and subscribe. DeepSeek-TUI-style runtime workbenches need a
single foreground push channel that covers the whole runtime surface.

## Scope

- Add an aggregate `GET /v1/events/stream` SSE endpoint.
- Support replay, bounded wait, and `follow=1` modes on the aggregate stream.
- Accept a per-thread cursor query, `since=thread-id:seq,...`, plus
  `since_seq=N` as the default cursor for unlisted threads.
- Emit aggregate SSE frames with `thread_id:seq` ids and the same runtime event
  JSON payload used by per-thread streams.
- Advertise aggregate SSE capabilities in `/runtime`.
- Change HTTP-runtime TUI live watching to follow the aggregate event stream and
  refresh snapshots after incoming events.
- Document the endpoint and update the parity plan.

## Acceptance

1. `GET /v1/events/stream?since_seq=0` replays events from multiple threads.
2. `GET /v1/events/stream?since=thread:seq&wait_ms=...` can wait for and return
   a newly created thread's `thread_created` event.
3. `follow=1` aggregate streams can close deterministically with `max_events`
   or `max_ms`.
4. HTTP-runtime TUI uses the aggregate stream instead of one startup-time
   subscription per known thread.
5. The TUI live watcher refreshes when a remote thread is created after startup.
6. Existing per-thread SSE replay/follow endpoints remain compatible.

## Implementation Notes

- Added `global_events_stream_response`, `handle_global_sse_follow_request`,
  per-thread cursor parsing, and aggregate event polling helpers in
  `src/cli/commands/serve.rs`.
- Added `/v1/events/stream`, `events_global_sse`, and
  `events_global_sse_follow` to runtime metadata.
- Reworked `start_runtime_http_live_watcher` in `src/cli/commands/tui.rs` to
  spawn one aggregate SSE worker that reconnects with an updated cursor map.
- Kept per-thread SSE frame ids unchanged; aggregate frames use `thread_id:seq`
  only on the aggregate endpoint.

## Verification

- `/home/willamhou/.cargo/bin/cargo test global_event_stream_endpoint --lib`
- `/home/willamhou/.cargo/bin/cargo test runtime_http_live_watcher_detects_new_remote_threads_from_global_sse --lib`
- `/home/willamhou/.cargo/bin/cargo test event_stream --lib`
- `/home/willamhou/.cargo/bin/cargo test runtime_http --lib`
- `/home/willamhou/.cargo/bin/cargo test tui --lib`
- `/home/willamhou/.cargo/bin/cargo test cli::commands::serve::tests:: --lib`
- `/home/willamhou/.cargo/bin/cargo fmt --check`
- `git diff --check`
- `/home/willamhou/.cargo/bin/cargo test`
- `/home/willamhou/.cargo/bin/cargo package --allow-dirty`
