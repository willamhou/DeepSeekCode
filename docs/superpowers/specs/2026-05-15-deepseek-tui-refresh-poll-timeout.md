# DeepSeek-TUI Refresh Poll Timeout Parity

## Source Comparison

- Upstream: `Hmbown/DeepSeek-TUI` `origin/main` at
  `b834548d3b1dd60d08f8023d64ba129945f44420`
- Upstream commit: `89e78d7 test(tui): avoid Windows Instant underflow`

## Gap

DeepSeek-TUI recently hardened a Windows-sensitive TUI timing test by avoiding
direct subtraction from a fresh `Instant`. DeepSeekCode's TUI loop already uses
`last_refresh.elapsed()` and `Duration::saturating_sub`, so it does not depend
on constructing an older `Instant`.

The missing part was regression coverage for the poll-timeout calculation. A
future refactor could accidentally reintroduce checked/unchecked `Instant`
arithmetic or an overlong blocking poll.

## Scope

- Keep the existing TUI loop behavior.
- Extract the event poll timeout calculation into a small pure helper.
- Add unit tests for:
  - long remaining refresh waits capped at 200 ms;
  - short remaining waits preserved exactly;
  - missed refresh intervals saturating to zero instead of underflowing.

## Non-Goals

- No TUI redraw scheduling redesign.
- No platform-specific sleeps or wall-clock timing tests.
- No change to the runtime event refresh contract.

## Acceptance

- `run_loop` uses the shared helper for `event::poll`.
- The helper never subtracts from an `Instant`; callers pass elapsed
  `Duration`.
- Focused unit tests cover capped, exact, and saturated timeout behavior.
- The broader TUI and library test suites remain green.

## Implementation

- `src/tui.rs`
  - added `tui_refresh_poll_timeout(refresh_interval, elapsed)`
  - wired `run_loop` through the helper
  - added three regression tests for timeout boundaries

