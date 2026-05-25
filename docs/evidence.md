# Evidence Summary

This page collects the public proof points for DeepSeekCode so launch posts and
integration PRs can link to one stable document instead of scattered command
snippets.

## Install Proof

Primary install paths:

```bash
npm install -g @deepseek-code/cli
deepseek version
deepseek quickstart
```

```bash
npx @deepseek-code/cli version
npx @deepseek-code/cli quickstart
```

Fallback install paths:

```bash
brew tap willamhou/deepseekcode
brew install deepseek
deepseek version
deepseek quickstart
```

The `v0.1.5` tap formula was published to
`willamhou/homebrew-deepseekcode` and Homebrew Smoke run `26380934049` verified
macOS x64 and macOS arm64 install plus `doctor --json`.

```bash
deepseek update download-plan --version 0.1.5
deepseek update release-smoke --version 0.1.5 --json
```

## Model-Backed Demo

The README shows a real interactive `deepseek chat` run that starts from an
empty disposable web repo and generates a playable 2048 game:

- terminal recording: [`docs/demo/deepseek-code-2048-interactive-demo.svg`](./demo/deepseek-code-2048-interactive-demo.svg)
- gameplay GIF: [`docs/demo/deepseek-code-2048-interactive-gameplay.gif`](./demo/deepseek-code-2048-interactive-gameplay.gif)
- gameplay MP4: [`docs/demo/deepseek-code-2048-interactive-gameplay.mp4`](./demo/deepseek-code-2048-interactive-gameplay.mp4)

Supplemental demos remain available for scripted 2048 capture, TUI surfaces,
TUI first-run UX guidance, and model-backed edit/test loops under
[`docs/demo/`](./demo/README.md).

## TUI UX Evidence

The TUI first-run evidence is a deterministic no-model capture against a
disposable small repo. It validates that `deepseek tui --once` surfaces setup
guidance for missing provider/model config, missing API key, unreviewed
workspace trust, and missing network policy:

- evidence SVG: [`docs/demo/deepseek-code-tui-ux-evidence.svg`](./demo/deepseek-code-tui-ux-evidence.svg)
- evidence log: [`docs/demo/deepseek-code-tui-ux-evidence.log`](./demo/deepseek-code-tui-ux-evidence.log)
- recorder: [`docs/demo/record-tui-ux-evidence.sh`](./demo/record-tui-ux-evidence.sh)

## Runtime Evidence

DeepSeekCode exposes runtime evidence without requiring users to inspect raw
`.dscode/runtime` JSON:

```bash
deepseek stats --json
deepseek events replay <thread-id> --limit 50
deepseek events diff <before-thread-id> <after-thread-id> --json
```

The deterministic repair/cache evidence command records a before/after pair of
runtime threads:

```bash
deepseek dogfood repair-cache-evidence --json
deepseek events replay <after-thread-id> --limit 50
deepseek events diff <before-thread-id> <after-thread-id>
deepseek stats --thread <after-thread-id> --require-prefix-stable
```

Expected proof:

- a truncated DeepSeek-style `read_file` argument object fails strict parsing
  before repair;
- the after thread records one structured `tool_call_repair` event;
- the after thread records prompt-layer cache diagnostics;
- failed tool calls drop from 1 to 0;
- cache hit rate increases from 0% to 75%.

More details: [`docs/dogfood-evidence.md`](./dogfood-evidence.md).

## External Fixtures

Tracked dogfood evidence covers disposable Python, Rust, and Node repositories:

- Python invoice multi-file fixture;
- Rust order multi-file fixture;
- Node task-report fixture.

The reusable fixture generator creates failing repositories outside this
checkout, then prints dry-run, evidence, and verification commands:

```bash
scripts/create-multifile-external-fixture.sh /tmp/deepseek-fixtures/python-invoice-multifile python-invoice-multifile --force
scripts/create-multifile-external-fixture.sh /tmp/deepseek-fixtures/rust-order-multifile rust-order-multifile --force
scripts/create-multifile-external-fixture.sh /tmp/deepseek-fixtures/node-task-report node-task-report --force
```

## Release Gates

Local release-readiness checks:

```bash
cargo fmt --check
cargo test --lib
npm --prefix npm test
node npm/scripts/check-version-sync.js
node scripts/check-secrets.js
deepseek update publish-status --json
```

Hosted release gates build release binaries for Linux x64, Linux arm64, macOS
x64, macOS arm64, and Windows x64. The release workflow also stages platform npm
packages and publishes the root npm wrapper when `NPM_TOKEN` is configured.
The `v0.1.5` Release Matrix run `26380726246` produced the GitHub Release
assets, published npm packages, updated the Homebrew tap, and pushed the GHCR
image.
The `v0.1.5` Release Smoke run `26380981708` verified public release archive
download, checksum, extraction, and install smoke on Linux x64, Linux arm64,
macOS x64, and macOS arm64.
For `v0.1.5`, registry and clean-machine smoke verified `deepseek 0.1.5`
through both `npx @deepseek-code/cli@0.1.5 version` and a clean-directory
`npm install @deepseek-code/cli@0.1.5`.
Homebrew Smoke run `26380934049` verified the published tap on macOS x64 and
macOS arm64.

## Known Limits

- Linux/macOS is the public-beta focus.
- Windows release assets exist, but Windows service-level proof is broader
  product hardening.
- Larger real external repositories would strengthen the evidence base.
- Hosted IDE evidence is not the current CLI milestone.
