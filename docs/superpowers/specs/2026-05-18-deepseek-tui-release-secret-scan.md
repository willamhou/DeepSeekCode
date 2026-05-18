# DeepSeek-TUI Release Secret Scan

## Source

Comparison source: `Hmbown/DeepSeek-TUI` refreshed at
`/tmp/deepseek-tui-compare-20260514`, `origin/main` at `eeccf7d`.

Real model-backed demo capture and release dogfood use API credentials. After
adding safer key-file/stdin handling and transcript redaction, the release path
still needed a simple tracked-file guard against accidentally committed tokens.

## Gap

The repository ignored `.env` and generated demo logs, but the release workflow
did not actively scan tracked files for obvious API key material before
packaging or publishing.

## Implemented Behavior

- Added `scripts/check-secrets.js`.
- The script scans repository text files for `sk-...` style API tokens while
  skipping generated/local secret paths, and reports masked file/line/column
  findings.
- Test fixtures can opt out per-line with `secret-scan: allow`.
- The Release Matrix packaging job runs the scanner before `cargo package`.
- README locale development checks and release docs include the scanner.
- The scanner is intentionally lightweight and local; it does not replace
  provider-side secret scanning.

## Validation

```bash
node --check scripts/check-secrets.js
node scripts/check-secrets.js
git diff --check
```

## Remaining

External repository secret scanning and revocation workflows remain provider
configuration tasks.
