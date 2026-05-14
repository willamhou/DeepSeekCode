# DeepSeek-TUI README Demo Recording Parity

**Status:** implemented on 2026-05-14
**Comparison source:** `Hmbown/DeepSeek-TUI` refreshed at `/tmp/deepseek-tui-compare-20260514`, HEAD `9483248a9f35b5f2b56c34b5b84fbc5334473c9d`.

## Gap

DeepSeek-TUI presents a stronger first-contact visual surface with README demo
media. DeepSeekCode only embedded a static deterministic TUI snapshot and still
told readers to record a demo later. That made the public README weaker after
the repository and `v0.1.0` release were published.

This slice closes the README-level media gap without claiming full launch-grade
model-backed screencast parity.

## Implementation

- Generate `docs/demo/deepseek-code-tui-demo.svg` as an animated SVG from
  `target/debug/deepseek tui --demo --once`.
- Keep `docs/demo/deepseek-code-tui.svg` as the static snapshot fallback.
- Update English, Simplified Chinese, and Japanese READMEs to embed the
  animated SVG and document the reproducible generation command.
- Update `docs/demo/README.md` so future demo regeneration has one canonical
  animated command and the existing static command.

## Verification

- `target/debug/deepseek tui --demo --once`
- `svg-term --command "bash -lc 'target/debug/deepseek tui --demo --once | sed -e \"s/^\\\"//\" -e \"s/\\\"$//\" | while IFS= read -r line; do printf \"%s\\n\" \"\$line\"; sleep 0.08; done; sleep 1.5'" --out docs/demo/deepseek-code-tui-demo.svg --width 122 --height 36 --window --no-cursor`
- `rg -o "animation-duration:[^;]+" docs/demo/deepseek-code-tui-demo.svg`
- `rg -n "deepseek-code-tui-demo|animated TUI demo|animated SVG" README.md README.zh-CN.md README.ja-JP.md docs/demo/README.md`
- `git diff --check`

## Residual Gap

The README now has an animated deterministic TUI recording. A real
model-backed screencast that shows request submission, edits, test execution,
and diff review is still a stronger future launch asset.
