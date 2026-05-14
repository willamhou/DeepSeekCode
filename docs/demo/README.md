# Demo Assets

`deepseek-code-tui-demo.svg` is the animated README demo generated from the
deterministic TUI snapshot:

```bash
svg-term --command "bash -lc 'target/debug/deepseek tui --demo --once | sed -e \"s/^\\\"//\" -e \"s/\\\"$//\" | while IFS= read -r line; do printf \"%s\\n\" \"\$line\"; sleep 0.08; done; sleep 1.5'" \
  --out docs/demo/deepseek-code-tui-demo.svg \
  --width 122 \
  --height 36 \
  --window \
  --no-cursor
```

`deepseek-code-tui.svg` is the static snapshot generated from the same
deterministic TUI output:

```bash
svg-term --command "bash -lc 'target/debug/deepseek tui --demo --once | sed -e \"s/^\\\"//\" -e \"s/\\\"$//\"; sleep 1'" \
  --out docs/demo/deepseek-code-tui.svg \
  --width 122 \
  --height 36 \
  --window \
  --no-cursor \
  --at 1000
```

For a launch-quality README, add a short GIF or MP4 that shows the real coding
loop: open the TUI, submit a request, apply an edit, run tests, and inspect the
diff.
