# DeepSeek-TUI Web Run Open Viewport Parity Spec

日期：2026-05-13

对比对象：`Hmbown/DeepSeek-TUI`，`main` HEAD `3382242`

## 背景

DeepSeek-TUI renders `web.run.open` and `web.run.click` as a line-windowed page
view. Requests can pass `lineno`, and the top-level `response_length` controls
the size of the returned window. The output includes line range metadata while
the cached page remains available for later `find`, `click`, and PDF
`screenshot`.

DeepSeekCode currently accepts `open`, `click`, and `response_length` fields but
returns the whole decoded body for opened pages. That is workable for small
pages, but it is less compatible with DeepSeek-TUI's long-page browsing loop.

## 目标

- Parse top-level `response_length=short|medium|long` for aggregate `web_run`.
- Honor per-open and per-click `lineno`, defaulting to line 1.
- Render `open` and `click` content as a line-numbered window.
- Include `meta.line_start`, `meta.line_end`, and `meta.total_lines` for page
  views.
- Keep the cached page text complete so `find`, later `click`, and PDF
  `screenshot` still see the full page.

## 非目标

- This slice does not implement full upstream wrapping width behavior.
- This slice does not change direct `fetch_url` output.
- This slice does not alter search/image result ranking or max-result defaults.

## 验收标准

1. `web_run.open` with `lineno` starts output near the requested line.
2. `response_length=short|medium|long` changes the returned page window size.
3. `web_run.click` applies the same line-window behavior.
4. Cached full page text is still searched by `find`.
5. Runtime docs, model schema, and parity plan describe the viewport behavior.

## 实现结果

- Added DeepSeek-TUI-style `response_length` parsing for aggregate `web_run`.
- Added `short=40`, `medium=80`, and `long=160` line-window sizing for opened
  and clicked pages.
- `web_run.open` and `web_run.click` now honor per-action `lineno`, defaulting
  to line 1.
- Page view output now includes `meta.line_start`, `meta.line_end`, and
  `meta.total_lines`.
- Cached page text remains complete while the returned page view is windowed, so
  later `find`, `click`, and PDF `screenshot` still operate on cached state.
- Updated model schema, runtime docs, and the DeepSeek-TUI parity plan.

## 验证

- `cargo fmt`: passed.
- `cargo test web_run_open_honors_lineno_and_response_length`: passed.
- `cargo test web_run_open_window_keeps_full_page_cached_for_find`: passed.
- `cargo test web_run_click_honors_lineno_window`: passed.
- `cargo test web_run`: passed, 16 tests.
- `cargo test build_tool_specs_include_web_search_and_fetch_url`: passed.
- `cargo test`: passed, 1019 tests.
- `cargo fmt --check`: passed.
- `git diff --check`: passed.
- `cargo package --allow-dirty`: passed, packaged 302 files.
