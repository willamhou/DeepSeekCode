# DeepSeek-TUI Web Run Click Parity Spec

日期：2026-05-13

对比对象：`Hmbown/DeepSeek-TUI`，`main` HEAD `3382242`

## 背景

DeepSeek-TUI `web.run` supports `click` against stored page refs: the model can
open a page, inspect its numbered links, then call `click` with `ref_id` and
`id` to fetch the target page. DeepSeekCode currently reports `click` as
unsupported, which leaves the aggregate web state machine short of the upstream
browser-navigation surface.

## 目标

- Cache opened pages as text plus numbered links.
- Extract links from HTML `<a href=...>` elements, including relative URLs
  resolved against the opened page URL.
- Include numbered links in open/fetch output so models can see valid ids.
- Add `web_run.click` support with upstream-compatible fields: `ref_id` and
  `id`.
- Fetch clicked link targets, cache them as `clickN` and `turnNclickN`, and
  expose those refs for later `find` / `open` / `click`.
- Keep `screenshot` unsupported in this slice.

## 非目标

- This slice does not execute JavaScript, maintain cookies, submit forms, or
  emulate a DOM browser.
- This slice does not add screenshot rendering.
- This slice does not preserve frame/window history beyond cached page refs.

## 验收标准

1. `web_run` no longer reports `click` as unsupported.
2. Opening an HTML page exposes numbered links.
3. `click` on a cached `openN` ref fetches the selected link target.
4. Relative links are resolved against the opened page URL.
5. Invalid link ids produce a clear error.
6. Clicked pages are cached and can be used by `find`.
7. Runtime docs, model schema, and parity plan describe the support boundary.

## 实现结果

- Upgraded `web_run` page cache from plain text to text plus numbered links.
- `FetchUrlTool` now extracts static HTML `<a href=...>` links, resolves
  relative URLs against the fetched page, and includes a `links:` section in
  output when links are present.
- Added `web_run.click` support with `ref_id` and `id` / `link_id`.
- Clicked links are fetched through the same safe `fetch_url` path and cached
  as `clickN` and `turnNclickN` refs for later `find`, `open`, or `click`
  actions.
- Invalid page refs and missing link ids return explicit errors.
- `click` was removed from the unsupported action list; `screenshot`,
  `weather`, `sports`, and `time` remain unsupported in this slice.
- Updated model schema, runtime docs, and the DeepSeek-TUI parity plan.

## 验证

- `cargo fmt`: passed.
- `cargo test web_run_open_exposes_links_and_click_fetches_target`: passed.
- `cargo test web_run_click_rejects_missing_link_id`: passed.
- `cargo test normalize_link_url_resolves_relative_links`: passed.
- `cargo test web_run`: passed, 11 tests.
- `cargo test build_tool_specs_include_web_search_and_fetch_url`: passed.
- `cargo test`: passed, 1013 tests.
- `cargo fmt --check`: passed.
- `git diff --check`: passed.
- `cargo package --allow-dirty`: passed, packaged 300 files.
