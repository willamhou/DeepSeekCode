# DeepSeek-TUI OpenAI Batch Tool Call Parity

## Source

Comparison source: `Hmbown/DeepSeek-TUI` refreshed at
`/tmp/deepseek-tui-compare-20260514`, `origin/main` after `eeccf7d`.

Relevant upstream fix: `a528ea9 fix(streaming): preserve all tool_calls in
OpenAI batch responses (#1686)`.

## Gap

DeepSeekCode asked OpenAI-compatible providers for serial tool execution with
`parallel_tool_calls:false`, but the streaming parser still assumed only one
tool call could arrive in a response. If a compatible gateway ignored that flag
and emitted multiple indexed `tool_calls`, DeepSeekCode kept only the first
tool call and dropped the rest.

That was a real code-agent gap: lost tool calls mean the next model turn sees an
incomplete observation set and may repeat work, miss a read, or skip validation.

## Implemented Behavior

- Model protocol now has a batch action for same-turn tool calls while retaining
  the original single `CallTool` action.
- OpenAI streaming now assembles tool calls by `tool_calls[].index`, preserving
  all names and argument chunks before producing an action.
- OpenAI non-streaming chat completion parsing now preserves every entry in
  `message.tool_calls`.
- The agent loop executes a batch sequentially in the same model turn, running
  the normal cancellation, hook, permission, repeat-guard, tool result,
  recovery-hint, replan-hint, and post-tool-hook paths for each call.
- Existing single-tool responses still use the original single-tool action, so
  the common path remains unchanged.

## Remaining Difference

DeepSeekCode executes same-turn batches sequentially rather than truly in
parallel. That is intentional for now because the local tools share workspace
state, permission policy, and hook side effects. The important parity closure is
that no OpenAI-compatible batch tool call is silently lost.

## Validation

- OpenAI stream parser regression: multiple indexed tool calls become a batch
  and emit two stream tool-call events.
- Agent loop regression: one model response with two tool calls records two
  tool events and passes both observations into the next model request.
- Existing loop guard behavior already records repeated blocked calls as failed
  tool events, matching the recent DeepSeek-TUI loop-guard accounting fix.

