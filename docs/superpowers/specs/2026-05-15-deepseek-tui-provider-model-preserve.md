# DeepSeek-TUI Provider Model Preservation

Source comparison: `Hmbown/DeepSeek-TUI` `f8a4dee fix(tui): preserve provider-selected models`.

## Problem

DeepSeekCode already had an offline TUI provider/model picker, but the picker
always reopened on its stored picker indices and reset the model index to the
first preset model whenever the provider changed. Reselecting the active
provider could therefore overwrite a workspace's current model with the preset
default, especially for provider-native ids or custom OpenAI-compatible models.

## Scope

- Open the provider picker on the selected workspace's inferred current
  provider by reading `.dscode/config.toml` plus supported env overrides.
- Select the matching provider model row when the current model maps to a known
  DeepSeek V4 alias or provider-native id.
- Preserve an active provider's current custom model on Enter when no picker row
  represents that model, instead of silently queueing the first preset model.
- Keep explicit provider/model navigation authoritative: once the user moves in
  the model pane, the selected row is the queued model.

## Acceptance

- A workspace configured for NVIDIA NIM with
  `deepseek-ai/deepseek-v4-flash` opens the picker on `nvidia-nim` / `flash`.
- A workspace configured for OpenAI-compatible `gpt-5.4` opens the picker on
  `openai` and queues `gpt-5.4` when the active provider is reselected.
- Existing direct `provider <name> [model]`, `provider show`, and
  `provider list` command paths remain unchanged.

## Verification

- `/home/willamhou/.cargo/bin/cargo test provider_picker_ --lib`
