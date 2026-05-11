# Scriptable Exec CLI

`deepseek exec` is the non-interactive CLI entrypoint for scripts, CI, and
repeatable dogfood runs. It leaves `deepseek run` compatible while adding a
stable input and output contract.

## Basic Use

Run a one-shot task:

```sh
deepseek exec "inspect the repository and summarize the main entrypoints"
```

Read the prompt from stdin:

```sh
cat task.txt | deepseek exec -
```

Limit agent loop steps:

```sh
deepseek exec --budget 12 "fix the failing tests"
```

Use a skill:

```sh
deepseek exec --skill debug "investigate the failing parser test"
```

Attach image file references:

```sh
deepseek exec --image screenshot.png "inspect the UI state"
```

`--image`/`-i` validates that each path exists, accepts `jpg`, `jpeg`, `png`,
`gif`, and `webp`, and includes explicit file references in the task text. When
the configured provider/model is recognized as vision-capable, OpenAI-compatible
requests send `image_url` data URLs and Anthropic-compatible requests send
base64 `image` content blocks. DeepSeek text-only profiles keep the file
references without native image payloads.

## JSONL Output

Add `--json` for newline-delimited JSON events:

```sh
deepseek exec --json "inspect the repository"
```

JSONL events are emitted live as the agent runs. Current event types:

- `session_started`
- `assistant_delta`
- `tool_call`
- `permission_request`
- `tool_result`
- `assistant_final`
- `error`

Each line is a complete JSON object. Scripts should branch on `type` instead of
scraping human-oriented progress text. `assistant_delta`, `tool_call`,
`permission_request`, and `tool_result` stream during execution; `assistant_final`
is emitted after the run completes with aggregate usage and tool-call counts.

## Resume

Continue the latest saved non-interactive session:

```sh
deepseek exec resume --last "apply the fix you described"
```

Continue a specific session:

```sh
deepseek exec resume session-123 "apply the fix you described"
```

Read the follow-up prompt from stdin:

```sh
cat followup.txt | deepseek exec resume --last -
```

Resume with image references:

```sh
deepseek exec resume --last --image screenshot.png "continue from this UI state"
```

If no follow-up prompt is provided, `exec resume` reruns the saved task from the
selected session snapshot.
