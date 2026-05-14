# DeepSeek-TUI Terminal Display Width Clipping

**Status:** implemented
**Comparison source:** `Hmbown/DeepSeek-TUI` refreshed at `/tmp/deepseek-tui-compare-20260514`; latest fetched `origin/main` `13e7957621448792beda06ec8615e33cb374adce`.

## Gap

The latest DeepSeek-TUI refresh added regression coverage for streaming text
that contains no whitespace, including long CJK runs and first-token overflow.
DeepSeekCode's TUI clipped many previews by character count. That is too weak
for terminal layout because CJK and other wide glyphs can occupy two cells and
overflow panel budgets even when the character count is below the limit.

## Implementation

- Added a direct `unicode-width` dependency.
- Changed `clip_line` to clip by terminal display width rather than character
  count.
- Counts the ellipsis inside the width budget so clipped previews stay inside
  their caller-provided panel limits.
- Added regression coverage for CJK text, long no-whitespace ASCII tokens, and
  unchanged short text.

## Verification

- `/home/willamhou/.cargo/bin/cargo fmt --check`
- `/home/willamhou/.cargo/bin/cargo test clip_line --lib`
- `/home/willamhou/.cargo/bin/cargo check`
- `/home/willamhou/.cargo/bin/cargo test --lib -- --test-threads=1`
- `git diff --check`

## Remaining

This closes the width-aware clipping slice. It does not replace every wrapped
paragraph renderer with a custom hard-wrap algorithm, and it does not prove
Windows terminal behavior.
