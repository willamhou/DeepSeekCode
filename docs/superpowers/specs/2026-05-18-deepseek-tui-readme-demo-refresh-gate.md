# DeepSeek-TUI README Demo Refresh Gate

## Source

Comparison source: `Hmbown/DeepSeek-TUI` refreshed at
`/tmp/deepseek-tui-compare-20260514`, `origin/main` at `eeccf7d`.

DeepSeek-TUI's public README makes the terminal product obvious on first
contact. DeepSeekCode already embeds an animated deterministic TUI SVG, but the
generation command lived in README prose and had no guard that the result stayed
animated.

## Gap

The user-facing request to re-record the README demo should not rely on copying
a long `svg-term` command by hand. A bad regeneration can silently replace the
animated SVG with a static frame unless a verifier checks the artifact.

## Implemented Behavior

- `docs/demo/record-readme-demo.sh` regenerates both README SVG assets from
  `deepseek tui --demo --once`.
- The script resolves `DEEPSEEK_DEMO_BIN`, PATH `deepseek`, or builds
  `target/debug/deepseek` when needed.
- The script requires `svg-term` and fails closed when the animated SVG is
  missing `@keyframes`, `animation-duration`, or `DeepSeekCode`.
- The static fallback is checked for `DeepSeekCode` and for absence of
  animation keyframes.
- English, Simplified Chinese, Japanese, and demo docs point to the recorder
  instead of repeating the long generation command.

## Validation

```bash
bash -n docs/demo/record-readme-demo.sh
docs/demo/record-readme-demo.sh
rg -n "@keyframes|animation-duration" docs/demo/deepseek-code-tui-demo.svg
git diff --check
```

## Remaining

This closes deterministic README demo regeneration. The stronger launch asset is
still a reviewed model-backed GIF/MP4/SVG generated from a successful online
fixture run.
