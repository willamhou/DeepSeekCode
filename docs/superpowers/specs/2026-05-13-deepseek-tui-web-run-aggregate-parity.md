# DeepSeek-TUI Web Run Aggregate Parity Spec

日期：2026-05-13

对比对象：`Hmbown/DeepSeek-TUI`，`main` HEAD `3382242`

## 背景

DeepSeek-TUI exposes a `web.run`-style aggregate web tool with action arrays
such as `search_query`, `open`, `find`, `screenshot`, `image_query`, and
`finance`. DeepSeekCode currently has separate `web_search`, `fetch_url`, and
`finance` tools. That is useful but leaves a large tool-call compatibility gap
for models that emit aggregate `web.run` payloads.

## 目标

- Add an agent-visible `web_run` aggregate tool.
- Support `search_query` arrays by delegating each query to the existing
  `web_search` implementation.
- Support direct URL `open` requests by delegating to `fetch_url`.
- Support URL-scoped `find` requests by fetching the URL and returning text
  snippets that match `pattern`.
- Support `finance` arrays by delegating each quote request to `finance`.
- Preserve the existing network safety model, local-host blocking, configurable
  templates, max byte limits, and timeout bounds.
- Expose `web_run` in the default registry and DeepSeek model schema.
- Explicitly report unsupported browser-state actions rather than silently
  ignoring them.

## 非目标

- This slice does not preserve browser page state or search result `ref_id`
  mappings across tool calls.
- This slice does not implement `click`, `screenshot`, `image_query`,
  `weather`, `sports`, or `time`.
- This slice does not render pages or execute JavaScript.

## 验收标准

1. `web_run search_query=[{"q":"..."}]` returns search results using existing
   `web_search` behavior.
2. `web_run open=[{"ref_id":"https://..."}]` fetches direct URLs using
   existing `fetch_url` behavior.
3. `web_run find=[{"ref_id":"https://...","pattern":"..."}]` fetches direct
   URLs and returns matching snippets.
4. `web_run finance=[{"ticker":"AAPL","type":"equity"}]` returns quote output
   using existing `finance` behavior.
5. Unsupported aggregate actions are listed in metadata/output.
6. The default registry exposes `web_run`.
7. The model schema exposes `web_run`.

## 实现结果

- Added `WebRunTool` in `src/tools/web.rs`.
- `web_run search_query=[...]` delegates each object to the existing
  `web_search` path, preserving search templates, timeout bounds, result
  parsing, and local/private host blocking.
- `web_run open=[...]` supports direct `http://` and `https://` URLs through
  `ref_id` or `url`, then delegates to `fetch_url`.
- `web_run find=[...]` supports direct URL + `pattern`, fetches the URL through
  the existing safe fetch path, strips HTML when appropriate, and returns
  bounded snippets.
- `web_run finance=[...]` delegates each quote request to the existing
  `finance` path.
- Unsupported browser-state or external-provider actions are surfaced through
  `meta.unsupported_actions` and an explanatory output line.
- Registered `web_run` in the default registry, model schema, runtime docs, and
  parity plan.

## 验证

- `cargo test web_run`: passed, 5 tests.
- `cargo test build_tool_specs_include_web_search_and_fetch_url`: passed.
- `cargo test default_registry_includes_read_only_git_history_tools`: passed.
- `cargo test tool_search`: passed, 4 tests.
- `cargo test`: passed, 987 tests.
- `cargo fmt --check`: passed.
- `git diff --check`: passed.
- `cargo package --allow-dirty`: passed; packaged 292 files and verified.
