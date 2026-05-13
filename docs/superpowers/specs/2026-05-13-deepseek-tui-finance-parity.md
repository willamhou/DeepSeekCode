# DeepSeek-TUI Finance Tool Parity Spec

日期：2026-05-13

对比对象：`Hmbown/DeepSeek-TUI`，`main` HEAD `3382242`

## 背景

DeepSeek-TUI exposes a read-only `finance` tool for live stock, ETF, index, and
crypto quote lookup. DeepSeekCode has generic web search/fetch tools, but no
finance-specific quote surface.

## 目标

- Add read-only agent-visible `finance`.
- Accept `ticker` or `symbol`, plus optional `type`, `market`, and
  `timeout_ms`.
- Normalize common crypto inputs like `BTC` with `type=crypto` to Yahoo-style
  `BTC-USD`.
- Fetch from a Yahoo Finance-compatible quote endpoint by default.
- Support `DSCODE_FINANCE_URL_TEMPLATE` for deterministic tests and private
  mirrors.
- Parse common quote fields into a concise bounded summary.
- Expose the tool through registry and model schemas.

## 非目标

- This slice does not add chart/history output.
- This slice does not guarantee availability of upstream public finance APIs.
- This slice does not provide financial advice or recommendations.

## 验收标准

1. `finance ticker=<symbol>` validates ticker input and fetches one quote.
2. `finance symbol=<symbol>` works as an alias.
3. Empty or unsafe symbols are rejected.
4. Crypto hints convert common bare symbols to `<symbol>-USD`.
5. Valid quote JSON returns symbol, name, price, change, percent, currency, and
   exchange when available.
6. Missing quote results return a clear error.
7. Registry and model schemas include `finance`.

## 实现结果

- Added `FinanceTool` in `src/tools/web.rs`.
- `finance` accepts `ticker` or `symbol`, optional `type`, `market`, and
  `timeout_ms`.
- Tickers are uppercased and restricted to common market-symbol characters:
  letters, numbers, `.`, `-`, `^`, and `=`.
- Common crypto calls such as `symbol=btc type=crypto` normalize to `BTC-USD`.
- The default endpoint is Yahoo Finance-compatible
  `https://query1.finance.yahoo.com/v7/finance/quote?symbols=<ticker>`.
- `DSCODE_FINANCE_URL_TEMPLATE` can replace the default endpoint for tests or
  private mirrors; `{ticker}` and `{symbol}` placeholders are supported.
- Quote JSON is parsed into a concise summary with symbol, name, price,
  change, percent change, currency, exchange, and market state when available.
- Registered `finance` in the default runtime registry and MCP/ACP read-only
  tool definitions.
- Added static model schemas for `finance`.
- Documented the tool in `docs/runtime.md` and the parity plan.

## 验证

- `/home/willamhou/.cargo/bin/cargo test finance` passed: 5 matching tests.
- `/home/willamhou/.cargo/bin/cargo test build_tool_specs_include_finance` passed.
- `/home/willamhou/.cargo/bin/cargo test default_registry_includes_read_only_git_history_tools` passed.
- `/home/willamhou/.cargo/bin/cargo test mcp_tools_list` passed: 4 matching tests.
- `/home/willamhou/.cargo/bin/cargo fmt --check` passed.
- `git diff --check` passed.
- `/home/willamhou/.cargo/bin/cargo test` passed: 961 tests.
- `/home/willamhou/.cargo/bin/cargo package --allow-dirty` passed: packaged
  280 files and verified `deepseek_code v0.1.0`.
