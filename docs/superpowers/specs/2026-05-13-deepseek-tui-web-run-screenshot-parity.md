# DeepSeek-TUI Web Run Screenshot Parity Spec

日期：2026-05-13

对比对象：`Hmbown/DeepSeek-TUI`，`main` HEAD `3382242`

## 背景

DeepSeek-TUI 的 `web.run` schema includes `screenshot`, but its implementation
does not run a browser screenshot pipeline. It only supports screenshot-style
page extraction for already-opened PDF pages: `open` fetches and stores PDF page
text, then `screenshot` returns the requested `pageno` from the cached PDF.

DeepSeekCode currently reports `screenshot` as unsupported, so models that
follow the DeepSeek-TUI web workflow can open PDFs but cannot request a specific
PDF page through the aggregate web state machine.

## 目标

- Detect opened PDF responses by content type or `.pdf` URL suffix.
- Extract PDF text through a local `pdftotext` dependency when a PDF is opened.
- Cache PDF page text alongside the existing `web_run` page cache.
- Add `web_run.screenshot` with upstream-compatible `ref_id` and optional
  `pageno` fields.
- Return clear errors for unknown refs, non-PDF pages, and out-of-range pages.
- Keep browser/DOM screenshots explicitly out of scope.

## 非目标

- This slice does not render bitmap screenshots.
- This slice does not execute JavaScript, preserve browser cookies, or capture
  full browser viewport state.
- This slice does not add PDF OCR; scanned image-only PDFs require OCR tooling
  before useful text can be returned.

## 验收标准

1. `web_run` no longer reports `screenshot` as unsupported.
2. Opened PDF pages are cached with page text.
3. `screenshot` returns `ref_id`, `pageno`, `total_pages`, and content for the
   requested cached PDF page.
4. Non-PDF refs and out-of-range pages produce clear errors.
5. Model schema, runtime docs, and parity plan describe the PDF-only boundary.
6. Focused tests cover cached PDF screenshot behavior and error handling.

## 实现结果

- Added PDF detection for fetched/opened pages using `Content-Type:
  application/pdf` or `.pdf` URL suffixes.
- Added `pdftotext` / Poppler-backed PDF text extraction and form-feed page
  splitting for cached web pages.
- Upgraded the `web_run` cached page structure to store text, links, and
  optional PDF page text.
- Added `web_run.screenshot` with `ref_id` plus optional zero-based `pageno`.
- `screenshot` now returns metadata for `ref_id`, `pageno`, `total_pages`, and
  the requested cached PDF page content.
- Non-PDF cached refs, unknown refs, and out-of-range page requests return
  explicit errors.
- Updated model schema, runtime docs, install notes, and the DeepSeek-TUI
  parity plan with the PDF-only support boundary.

## 验证

- `cargo test web_run_screenshot`: passed, 2 tests.
- `cargo test split_pdf_text_pages`: passed, 1 test.
- `cargo test build_tool_specs_include_web_search_and_fetch_url`: passed.
- `cargo test web_run`: passed, 13 tests.
- `cargo fmt`: passed.
- `cargo test`: passed, 1016 tests.
- `cargo fmt --check`: passed.
- `git diff --check`: passed.
- `cargo package --allow-dirty`: passed, packaged 301 files.
