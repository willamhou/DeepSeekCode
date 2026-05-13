# DeepSeek-TUI Image Analyze Parity Spec

日期：2026-05-13

对比对象：`Hmbown/DeepSeek-TUI`，`main` HEAD `3382242`

## 背景

DeepSeek-TUI exposes a vision module with an agent-visible `image_analyze`
tool. The tool reads a workspace image, base64-encodes it as a data URL, sends
it to an OpenAI-compatible vision `/chat/completions` endpoint, and returns a
JSON object with `analysis` and `model`.

DeepSeekCode already supports CLI image attachments through `deepseek exec
--image` for vision-capable profiles, but the model-visible tool catalog does
not include `image_analyze`. Tool calls using the DeepSeek-TUI name therefore
fail today.

## 目标

- Add an `image_analyze` tool with upstream-compatible fields:
  required `image_path` and optional `prompt`.
- Support PNG, JPEG/JPG, GIF, WebP, and BMP MIME detection.
- Keep image paths workspace-relative and reject absolute paths, `..`
  traversal, missing files, and directories before any API call.
- Send an OpenAI-compatible `messages[].content[]` payload containing text and
  `image_url` data URL parts.
- Use configurable vision model settings while allowing per-call
  `model`, `base_url`, and `api_key_env` overrides for smoke tests or
  alternate gateways.
- Return upstream-compatible JSON containing `analysis` and `model`.
- Expose the tool in the default registry and model schema as read-only.

## 非目标

- This slice does not add a browser screenshot or DOM state pipeline.
- This slice does not add Anthropic-native image payloads to the tool; it uses
  the same OpenAI-compatible endpoint shape as DeepSeek-TUI.
- This slice does not install or validate external vision providers.

## 验收标准

1. `image_analyze` appears in the default registry and model schema.
2. `image_analyze` rejects absolute paths and parent-directory traversal before
   reading files.
3. `image_analyze` rejects unsupported image extensions before any API call.
4. MIME detection covers PNG, JPEG/JPG, GIF, WebP, and BMP.
5. Response parsing extracts `choices[0].message.content` and model id.
6. Missing API key errors name the configured `api_key_env`.
7. Runtime docs mention the tool and its vision config knobs.

## 实现结果

- Added `VisionConfig` to `AppConfig` with default OpenAI-compatible vision
  settings and config/env overrides:
  `vision.base_url`, `vision.model`, `vision.api_key_env`, plus
  `vision_model.*` aliases and `DSCODE_VISION_*` / `DEEPSEEK_VISION_*`
  environment overrides.
- Added `src/tools/vision.rs` with `ImageAnalyzeTool`.
- `image_analyze` accepts `image_path` and optional `prompt`; `path` is
  accepted as a compatibility alias but the model schema advertises
  `image_path`.
- The tool rejects unsafe workspace paths, missing files, directories, and
  unsupported extensions before reading image bytes or requiring an API key.
- Implemented built-in base64 encoding and MIME detection for PNG, JPEG/JPG,
  GIF, WebP, and BMP without adding dependencies.
- The tool sends an OpenAI-compatible `/chat/completions` request through
  `curl` with text and `image_url` data URL content parts, parses
  `choices[0].message.content`, and returns upstream-compatible
  `{"analysis": "...", "model": "..."}` JSON.
- Registered `image_analyze` in the default registry and model schema as a
  read-only tool.
- Documented `image_analyze` and `vision.*` settings in runtime and install
  docs, and updated the DeepSeek-TUI parity plan.
- Follow-up MCP work exposes `image_analyze` through MCP/ACP only in trusted
  side-effect or durable approval modes because it can spend model tokens and
  use networked vision APIs.

## 验证

- `cargo fmt`: passed.
- `cargo test image_analyze`: passed, 5 tests.
- `cargo test vision`: passed, 10 tests.
- `cargo test build_tool_specs_include_document_tools`: passed.
- `cargo test default_registry_includes_read_only_git_history_tools`: passed.
- `cargo test init_config_creates_project_bootstrap_files`: passed.
- `cargo test`: passed, 1008 tests.
- `cargo fmt --check`: passed.
- `git diff --check`: passed.
- `cargo package --allow-dirty`: passed; packaged 298 files.
