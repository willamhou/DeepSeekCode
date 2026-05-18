# DeepSeek-TUI Entrypoint PTY Smoke Proof

## Source

Comparison source: `Hmbown/DeepSeek-TUI` refreshed at
`/tmp/deepseek-tui-compare-20260514`, `origin/main` at `eeccf7d`.

DeepSeek-TUI's public dispatcher makes bare `deepseek` the terminal workbench
entrypoint. DeepSeekCode now routes bare `deepseek` to the TUI in real TTYs, but
that behavior still needed repeatable release evidence instead of one-off manual
PTY checks.

## Gap

`deepseek tui --demo --once` proves deterministic rendering, but it does not
prove raw terminal startup, alternate-screen entry/exit, or keyboard quit
handling through a real PTY. The gap matters because the default public command
is now a full-screen terminal app.

## Implemented Behavior

- `deepseek tui --entrypoint-smoke` starts the current executable as bare
  `deepseek` under the Unix `script` PTY wrapper or a Windows ConPTY-backed
  `cmd.exe` host.
- `--smoke-bin <path>` smokes a selected binary, which lets release gates target
  `./target/release/deepseek` or an installed binary.
- The Release Matrix and normal CI run that smoke directly against release or
  debug binaries before packaging or accepting `main`.
- Windows TUI default detection now treats available `CONIN$` / `CONOUT$`
  console devices as full-screen capable even when inherited stdio handles are
  not reported as terminals by Rust's `is_terminal`.
- Windows interactive TUI startup rebinds standard handles to those console
  devices before enabling raw mode, so bare `deepseek.exe` enters the full-screen
  workbench inside ConPTY-hosted environments.
- The smoke sends `q` to the PTY, verifies successful exit, checks alternate
  screen enter/leave sequences, and confirms the TUI rendered `DeepSeekCode`
  plus `TUI`.
- The command emits `deepseek.tui.entrypoint_smoke.v1` JSON with status,
  terminal takeover booleans, byte counts, and short output previews.
- Unsupported or failed PTY startup fails closed instead of silently passing.

## Validation

- Parser coverage for `--entrypoint-smoke`, `--smoke-bin`, and incompatible
  combinations.
- Unit coverage for terminal takeover report detection and shell-quoted binary
  paths.
- Manual/CI release command:

```bash
deepseek tui --entrypoint-smoke --smoke-bin "$(command -v deepseek)"
```

- GitHub Actions evidence:
  - `26013810423`: Linux, macOS x64, and Windows x64 CI all green after the
    Windows ConPTY entrypoint fix.
  - `26013911327`: README demo refresh CI all green with the same entrypoint
    smoke still enabled.
