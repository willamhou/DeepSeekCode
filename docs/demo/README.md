# Demo Assets

`deepseek-code-tui-demo.svg` is the animated README demo generated from the
deterministic TUI snapshot. `deepseek-code-tui.svg` is the static fallback from
the same snapshot.

```bash
docs/demo/record-readme-demo.sh
```

The recorder defaults to `target/debug/deepseek`, then PATH `deepseek`, then
builds the debug binary. It requires `svg-term` and fails if the animated SVG is
missing keyframes.

The committed `deepseek-code-model-demo.svg` is generated from a verified real
model-backed transcript. It shows the source-evidence loop: failing `cargo test`,
`deepseek exec`, a one-line Rust patch, and passing `cargo test`.

`record-2048-demo.sh` captures a more visual launch demo: an empty disposable
web repository, a model-backed `deepseek exec` run that builds a playable 2048
game with plain HTML/CSS/JS, file validation, `git diff --stat`, and an
optional local preview server for browser gameplay capture.

The committed `deepseek-code-2048-gameplay.gif` and
`deepseek-code-2048-gameplay.mp4` are generated from a reviewed real
model-backed 2048 transcript and browser gameplay capture.

## Model-Backed Demo Capture

Use `record-model-backed-demo.sh` to capture real model-backed CLI evidence
against a disposable Rust repository. The script creates a small failing crate,
shows the initial failing `cargo test`, runs `deepseek exec` with write/shell
approvals limited to that disposable repository, then records the final diff
and passing test output.

Dry-run the capture plan without requiring an API key:

```bash
docs/demo/record-model-backed-demo.sh --dry-run
docs/demo/record-model-backed-demo.sh --redaction-self-test
```

Record a real model-backed transcript:

```bash
printf '%s\n' '<deepseek-api-key>' > /tmp/deepseek-demo.key
chmod 600 /tmp/deepseek-demo.key
DEEPSEEK_DEMO_KEY_FILE=/tmp/deepseek-demo.key docs/demo/record-model-backed-demo.sh
latest_log=$(ls -t docs/demo/deepseek-code-model-demo-*.log | head -n 1)
docs/demo/verify-model-backed-demo.js "$latest_log"
docs/demo/render-model-backed-demo-svg.js "$latest_log" --out docs/demo/deepseek-code-model-demo.svg
```

The default output is a timestamped `docs/demo/deepseek-code-model-demo-*.log`
transcript. Verify the transcript before converting a reviewed successful run
into the GIF/MP4 or SVG asset linked from the README. The SVG renderer first
reuses the verifier, then extracts the failing test, `deepseek exec`, diff, and
passing test evidence into a static terminal-style asset. Do not publish runs
created with `DEEPSEEK_DEMO_ALLOW_OFFLINE=1` as model-backed evidence.

`DEEPSEEK_DEMO_KEY_FILE` must point outside this repository so API keys cannot
be accidentally committed. `--api-key-stdin` is also supported when piping from
a local secret manager. The transcript stream redacts known API key values
before writing the log.

The verifier can be checked without a model call:

```bash
docs/demo/verify-model-backed-demo.js --self-test
docs/demo/render-model-backed-demo-svg.js --self-test
```

## 2048 Launch Demo Capture

Dry-run the 2048 capture plan without creating a repo or spending model calls:

```bash
docs/demo/record-2048-demo.sh --dry-run
docs/demo/record-2048-demo.sh --redaction-self-test
```

Record a real model-backed transcript:

```bash
printf '%s\n' '<deepseek-api-key>' > /tmp/deepseek-2048.key
chmod 600 /tmp/deepseek-2048.key
DEEPSEEK_2048_KEY_FILE=/tmp/deepseek-2048.key docs/demo/record-2048-demo.sh
```

Record with a local preview server for GIF/MP4 capture:

```bash
DEEPSEEK_2048_KEY_FILE=/tmp/deepseek-2048.key docs/demo/record-2048-demo.sh --serve
```

The 2048 recorder defaults to `DEEPSEEK_2048_MODEL=deepseek-v4-pro`, a longer
model stream timeout, a larger per-turn output cap for code-generating tool
calls, and one retry because launch captures are expensive to restart:
`DSCODE_MODEL_STREAM_TIMEOUT_SECS=240`, `DSCODE_MODEL_MAX_TOKENS=4096`, and
`DEEPSEEK_2048_ATTEMPTS=2`.
It also validates the generated app with `node --check app.js`, required DOM
ids, linked assets, byte counts, `git status --short`, and `git diff --stat`.
The model process runs with an isolated `HOME` and a temporary `demo-2048` skill
that exposes only `list_files` and `write_file`, so local/user skill
auto-selection cannot change the demo tool surface or spend steps on unrelated
tools.

The script prints the disposable demo repo and transcript path. Keep raw
transcripts only after reviewing them for local paths and generated content
quality. Use `--cleanup` only after recording any browser gameplay you need.
