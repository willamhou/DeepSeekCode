# DeepSeek-TUI Pandoc And Image OCR Parity Spec

日期：2026-05-13

对比对象：`Hmbown/DeepSeek-TUI`，`main` HEAD `3382242`

## 背景

DeepSeek-TUI exposes optional local-dependency tools for document conversion and
OCR: `pandoc_convert` wraps the `pandoc` binary, and `image_ocr` wraps local
`tesseract`. DeepSeekCode currently has no matching tool names, so model calls
for these workflows fail even when the user has the binaries installed.

## 目标

- Add `pandoc_convert` with upstream-compatible fields:
  `source_path`, `target_format`, and optional `output_path`.
- Support the same curated target set: `markdown`, `gfm`, `commonmark`,
  `html`, `rst`, `latex`, `docx`, `odt`, `epub`, `plain`, `asciidoc`.
- Require `output_path` for binary formats `docx`, `odt`, and `epub`.
- Return text inline when `output_path` is omitted for text formats.
- Add `image_ocr` with upstream-compatible `path`.
- Resolve paths under the workspace and avoid arbitrary shell interpolation.
- Expose both tools in the default registry and model schema.
- Classify `pandoc_convert` as a write permission request only when
  `output_path` is present.

## 非目标

- This slice does not install `pandoc` or `tesseract`.
- This slice does not expose free-form pandoc arguments.
- This slice does not add multi-language OCR flags; users can still use shell
  tools for custom `tesseract` language packs or PSM modes.

## 验收标准

1. `pandoc_convert` rejects unsupported target formats before spawning pandoc.
2. `pandoc_convert` rejects binary output formats without `output_path`.
3. `pandoc_convert` returns clear missing-source and missing-binary errors.
4. `image_ocr` rejects missing paths before spawning tesseract.
5. `pandoc_convert` with `output_path` is classified as a write request.
6. `pandoc_convert` and `image_ocr` appear in the default registry and schema.

## 实现结果

- Added `src/tools/document.rs` with DeepSeek-TUI-compatible
  `pandoc_convert` and `image_ocr` wrappers.
- `pandoc_convert` accepts workspace-relative `source_path`,
  `target_format`, optional `output_path`, and optional `cwd`/`workspace`.
  It validates the upstream target allowlist, requires `output_path` for
  `docx`/`odt`/`epub`, refuses unsafe path traversal and symlink output
  targets, and runs local `pandoc` without shell interpolation.
- `image_ocr` accepts workspace-relative `path`, verifies the path exists and
  is a file, then runs local `tesseract <image> -` without shell
  interpolation.
- Both tools return explicit missing-binary messages when `pandoc` or
  `tesseract` is not available on `PATH`.
- Registered both tools in the default registry and model schema.
- Classified `pandoc_convert` as a write permission request only when
  `output_path` is non-empty; read-only inline conversions remain normal tool
  calls.
- Documented both tools in `docs/runtime.md` and updated the DeepSeek-TUI
  parity plan.

## 验证

- `cargo fmt`: passed.
- `cargo test pandoc_convert`: passed, 3 tests.
- `cargo test image_ocr`: passed, 1 test.
- `cargo test binary_format_detection_matches_upstream_set`: passed.
- `cargo test build_tool_specs_include_document_tools`: passed.
- `cargo test permission_request_for_reports_write_shell_and_mcp_prompts`:
  passed.
- `cargo test default_registry_includes_read_only_git_history_tools`: passed.
- `cargo test`: passed, 999 tests.
- `cargo fmt --check`: passed.
- `git diff --check`: passed.
- `cargo package --allow-dirty`: passed; packaged 296 files.
