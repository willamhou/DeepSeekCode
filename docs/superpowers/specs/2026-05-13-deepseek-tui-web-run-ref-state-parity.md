# DeepSeek-TUI Web Run Ref State Parity Spec

日期：2026-05-13

对比对象：`Hmbown/DeepSeek-TUI`，`main` HEAD `3382242`

## 背景

The first `web_run` slice added aggregate `search_query`, direct URL `open`,
URL-scoped `find`, and `finance`. DeepSeek-TUI's browser-style workflow also
lets later actions refer to prior results with IDs such as `search0` or
`turn0search0`. DeepSeekCode should support that common flow without adding a
full browser engine yet.

## 目标

- Store URLs returned by `web_run search_query` in process-local state.
- Expose and retain both compact refs such as `search0` and turn-scoped refs
  such as `turn0search0`.
- Let `web_run open` resolve stored search refs as well as direct URLs.
- Cache text content fetched by `web_run open`.
- Let `web_run find` search cached opened content by ref_id, or resolve a stored
  search ref and fetch it when no opened content exists.
- Preserve the existing network safety model and bounded outputs.

## 非目标

- This slice does not persist web refs across process restarts.
- This slice does not implement DOM clicks, screenshots, JavaScript execution,
  or image search.
- This slice does not guarantee ref IDs are stable across independent process
  launches.

## 验收标准

1. A `web_run search_query` call stores `search0` and a `turnNsearch0` alias for
   the first result URL.
2. A later `web_run open=[{"ref_id":"search0"}]` resolves and fetches the stored
   URL.
3. `web_run open` caches fetched text under an `openN` and `turnNopenN` ref.
4. `web_run find=[{"ref_id":"open0","pattern":"..."}]` searches cached opened
   text without requiring another fetch.
5. Direct URL behavior from the first `web_run` slice still works.

## 实现结果

- Added process-local `web_run` state for search-result URLs and opened-page
  text in `src/tools/web.rs`.
- `web_run search_query` now stores compact refs such as `search0` plus
  turn-scoped aliases such as `turnNsearch0`, and emits `web_run_ref` lines.
- `web_run open` resolves stored search refs or direct URLs, fetches through the
  existing safe `fetch_url` path, and caches rendered content under `openN`,
  `turnNopenN`, the source ref, and the URL.
- `web_run find` checks cached opened content first, then falls back to resolving
  a stored search ref or direct URL and fetching through the existing safe path.
- Updated the `web_run` schema and runtime docs to describe stored refs and the
  remaining unsupported browser actions.

## 验证

- `cargo test web_run`: passed, 7 tests.
- `cargo test build_tool_specs_include_web_search_and_fetch_url`: passed.
- `cargo test default_registry_includes_read_only_git_history_tools`: passed.
- `cargo test`: passed, 989 tests.
- `cargo fmt --check`: passed.
- `git diff --check`: passed.
- `cargo package --allow-dirty`: passed; packaged 293 files and verified.
