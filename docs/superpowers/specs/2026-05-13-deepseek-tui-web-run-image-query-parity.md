# DeepSeek-TUI Web Run Image Query Parity Spec

日期：2026-05-13

对比对象：`Hmbown/DeepSeek-TUI`，`main` HEAD `3382242`

## 背景

DeepSeek-TUI `web.run` supports `image_query` alongside `search_query`,
`open`, `click`, `find`, and `screenshot`. DeepSeekCode currently reports
`image_query` as unsupported, so model calls that ask for image search lose a
capability that the upstream TUI already exposes.

## 目标

- Add `web_run.image_query` support for arrays of objects with `q`, optional
  `max_results`, `timeout_ms`, `domains`, and `recency`.
- Use DuckDuckGo Images by default: fetch the seed page, extract the `vqd`
  token, then query the `i.js` JSON endpoint.
- Keep a `DSCODE_IMAGE_SEARCH_URL_TEMPLATE` override for deterministic tests
  and alternate gateways.
- Return image URL, thumbnail, title, source page URL, source, width, and
  height when present.
- Apply domain filters to source page URLs when `domains` is provided.
- Keep `click` and `screenshot` explicitly unsupported in this slice.

## 非目标

- This slice does not add a browser renderer or DOM session.
- This slice does not download image bytes or generate thumbnails locally.
- This slice does not enforce `recency`; it reports the same warning style as
  existing `search_query` compatibility paths.

## 验收标准

1. `web_run` no longer reports `image_query` as unsupported.
2. `image_query` returns structured image result lines from a configured JSON
   endpoint.
3. `image_query` applies `domains` filtering.
4. Invalid or tokenless default DuckDuckGo responses fail with clear errors.
5. Model schema describes `image_query` as supported.
6. Runtime docs and the parity plan mention the support boundary.

## 实现结果

- Added `web_run.image_query` handling in `src/tools/web.rs`.
- Implemented DuckDuckGo Images support by fetching the image seed page,
  extracting `vqd`, and querying the `i.js` JSON endpoint.
- Added `DSCODE_IMAGE_SEARCH_URL_TEMPLATE` for deterministic tests and
  alternate JSON image-search gateways.
- Parsed image result fields: `image`, `thumbnail`, `title`, source page
  `url`, `source`, `width`, and `height`.
- Applied optional `domains` filtering against source page URLs.
- Preserved compatibility warnings for non-enforced `recency`.
- Removed `image_query` from the unsupported `web_run` action list while
  leaving `click`, `screenshot`, `weather`, `sports`, and `time` explicitly
  unsupported.
- Updated the model schema, runtime docs, and DeepSeek-TUI parity plan.

## 验证

- `cargo fmt`: passed.
- `cargo test web_run_image_query`: passed, 2 tests.
- `cargo test web_run_reports_unsupported_actions`: passed.
- `cargo test build_tool_specs_include_web_search_and_fetch_url`: passed.
- `cargo test`: passed, 1010 tests.
- `cargo fmt --check`: passed.
- `git diff --check`: passed.
- `cargo package --allow-dirty`: passed; packaged 299 files.
