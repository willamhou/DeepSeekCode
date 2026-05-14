# DeepSeek-TUI Parity: TUI Logout Command

## Context

DeepSeek-TUI exposes `/logout` to clear persisted API-key state and return to
API-key onboarding. DeepSeekCode does not persist raw API keys in project config;
it references environment variable names such as `DEEPSEEK_API_KEY`. It does,
however, load a workspace `.env` file into the current process when values are
not already exported.

## Goals

- Add TUI `logout` / `/logout` before custom slash fallback.
- Clear the current TUI process values for the selected workspace's
  `model.api_key_env` and `vision.api_key_env` variables.
- Remove matching assignments from the selected workspace `.env` file while
  preserving unrelated lines.
- Render a detail panel that lists cleared env vars and removed `.env`
  assignments.
- State the parent-shell limitation explicitly: a child TUI process cannot unset
  variables exported by the shell that launched it.

## Acceptance

- `logout` / `/logout` queues a local TUI action.
- The local action removes current-process API key env vars.
- The local action edits only matching `.env` assignments and leaves unrelated
  entries intact.
- The detail panel reports exactly what was cleared or absent.
- Tests cover command queuing and local `.env` / process-env cleanup.
