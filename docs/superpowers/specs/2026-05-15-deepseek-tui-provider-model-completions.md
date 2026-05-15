# DeepSeek-TUI Provider-Aware Model Completions

Source comparison: `Hmbown/DeepSeek-TUI` `eef16f4 fix(model): canonicalize DeepSeek model completions`.

## Problem

DeepSeekCode already normalizes model changes according to the active provider,
so bare DeepSeek V4 aliases are persisted as provider-native ids for backends
such as NVIDIA NIM and OpenRouter. The composer slash completion list still
offered only fixed DeepSeek bare `/model deepseek-v4-*` suggestions, which made
the hint surface less accurate for compatible providers.

## Scope

- Generate composer `/model <name>` and `/config model <name>` completions from
  the selected workspace's active provider.
- Keep official DeepSeek endpoints on bare `deepseek-v4-*` model ids.
- Surface provider-native ids for NVIDIA NIM, OpenRouter, Novita, Fireworks,
  SGLang, vLLM, OpenAI-compatible, AtlasCloud, and Ollama presets.
- Keep model-setting behavior unchanged: `set_model_at` remains the final
  normalization gate before config writes.

## Acceptance

- DeepSeek base URL completions include bare `deepseek-v4-pro` and
  `deepseek-v4-flash`.
- NVIDIA NIM completions include `deepseek-ai/deepseek-v4-pro` and
  `deepseek-ai/deepseek-v4-flash`.
- OpenRouter completions include `deepseek/deepseek-v4-*`.
- The fixed composer completion list no longer hardcodes bare DeepSeek V4 ids;
  provider-specific entries come from runtime config.

## Verification

- `/home/willamhou/.cargo/bin/cargo test provider_model_completion_values_use_active_provider_ids --lib`
- `/home/willamhou/.cargo/bin/cargo test configure_tui_slash_completions --lib`
