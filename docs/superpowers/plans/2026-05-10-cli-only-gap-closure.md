# Claude/Codex CLI-Only Gap Closure Plan

最后更新：`2026-05-10`
状态：`implemented parallel subagent slice`
关联 spec：`docs/superpowers/specs/2026-05-10-cli-only-gap-audit.md`

## Goal

把 CLI-only residual gap 从实施前约 `22% - 28%` 压到 `<10%`。

CLI-12 首轮实现已把估计差距压到约 `12% - 16%`。后续补上 live JSONL、20 个 subagent benchmark cases、parallel subagent/thread management、MCP prompt slash commands、native image payloads、REPL `/compact` + `pre_compact` wiring、release package 和 clean install verifier 后，估计差距约为 `6% - 9%`。还不能标记完成，因为 100+ live dogfood 样本仍未完成。

本计划不包含 IDE workbench、Codex app/cloud、GitHub Action、Slack/Linear 等非 CLI 产品面。

## Phase CLI-12A - Scriptable CLI Contract

目标 residual gap：`22% - 28% -> 18% - 22%`
状态：`done`

Deliverables:

- Add `deepseek exec` as an explicit non-interactive entrypoint, or make `deepseek run --json` equivalent and documented.
- Support `PROMPT = "-"` to read stdin.
- Add JSONL event output:
  - session started
  - assistant delta / final
  - tool call
  - tool result
  - approval request / denied
  - error
- Add `deepseek exec resume [SESSION_ID|--last] [PROMPT|-]`.
- Add benchmark cases for stdin, JSONL, and resume follow-up.

Acceptance:

- CI can call one command and parse events without scraping human text.
- Resume works for non-interactive tasks.
- Existing `run` remains compatible.

## Phase CLI-12B - Subagent CLI Maturity

目标 residual gap：`18% - 22% -> 14% - 17%`
状态：`done except live dogfood depth`

Deliverables:

- Add custom subagent file format:
  - project: `.dscode/agents/*.md`
  - user: `~/.config/dscode/agents/*.md`
  - frontmatter: `name`, `description`, `tools`, optional `model`
- Add `/agents` or `deepseek agents` management:
  - list
  - show
  - validate
- Add explicit parallel subagent prompt handling:
  - require user request for parallelism
  - assign disjoint write scopes
  - collect child summaries
  - parent readback gate for child-edited files
- Expand subagent benchmark category to at least 20 cases.

Acceptance:

- Subagent work is inspectable from CLI.
- Parent does not silently trust child patches without readback.
- No unbounded nested dispatch.
- Parallel child runs produce thread artifacts and can be listed/switched from CLI.

## Phase CLI-12C - Hooks Event Parity

目标 residual gap：`14% - 17% -> 12% - 14%`
状态：`mostly done`

Deliverables:

- Add hook events:
  - `session_start`
  - `session_stop`
  - `permission_request`
  - `subagent_start`
  - `subagent_stop`
  - `pre_compact`
- Make pre-tool and permission hook outputs structured:
  - allow
  - deny
  - add_context
  - system_message
- Add hook benchmark fixtures.

Acceptance:

- Hook payloads are stable JSON.
- Blocking events are clearly separated from advisory events.
- Hook failures do not corrupt session state.
- REPL `/compact` triggers `pre_compact` before transcript rewrite.

## Phase CLI-12D - MCP Schema, Prompt, And Permission UX

目标 residual gap：`12% - 14% -> 10% - 13%`
状态：`mostly done`

Deliverables:

- Inject remote MCP input schema into dynamic tool definitions.
- Replace `arguments` wrapper for tools whose schema can be safely represented.
- Keep fallback wrapper for unsupported schemas.
- Render permission prompts with:
  - server
  - tool
  - arguments summary
  - source config path
- Expose MCP prompts as slash/custom commands if the server supports prompt discovery.
- Add benchmark cases for stdio/HTTP/SSE schema injection and allow/deny behavior.

Acceptance:

- Model sees first-class tool schema for common MCP tools.
- User sees exactly which remote tool is being called and why.
- MCP prompt discovery/get is available from both explicit CLI commands and REPL slash commands.
- Broken MCP servers are isolated.

## Phase CLI-12E - Model Context And Distribution

目标 residual gap：`10% - 12% -> 8% - 9%`
状态：`partially done`

Deliverables:

- Add CLI image input path if the configured model supports it.
- Add web/search strategy:
  - either first-class tool support
  - or documented MCP-backed web/search setup with benchmark coverage
- Add online-model dogfood gate:
  - separate offline benchmark from live model stability gate
  - classify transport failure vs agent failure
- Add `deepseek update` or release-manager equivalent.
- Add clean-machine install smoke:
  - install
  - config init
  - doctor
  - exec JSONL sample
  - benchmark sample

Acceptance:

- Online DeepSeek path is measured, not assumed.
- Users can install and update without reading source code.
- CLI-only gap can be credibly scored below `10%`.

## Prompt-To-Artifact Checklist

| Objective item | Artifact coverage |
|---|---|
| 1. 交互体验成熟度 | Spec gap section 1; Phase CLI-12A |
| 2. Subagent 成熟度 | Spec gap section 2; Phase CLI-12B |
| 3. Hooks 事件面 | Spec gap section 3; Phase CLI-12C |
| 4. MCP / tool schema UX | Spec gap section 4; Phase CLI-12D |
| 5. 模型与上下文能力 | Spec gap section 5; Phase CLI-12E |
| 6. 安装/升级/分发 | Spec gap section 6; Phase CLI-12E |

## Implementation Notes

Completed in CLI-12 initial slice:

- `deepseek exec` with stdin, live JSONL output, `resume --last`, skill/budget, and image file references.
- `deepseek agents list/show/validate` plus project/user custom agent files.
- `dispatch_subagents` for up to 4 concurrent child tasks, consolidated summaries, and `.dscode/agent-threads/*.md` artifacts.
- `deepseek agents threads/show-thread/switch/current/clear-current` for thread inspection and switching.
- 20 `subagent` benchmark cases and a passing subagent-only verifier run.
- Expanded hook events and structured hook decisions.
- Dynamic MCP schema cache/injection and argument-aware permission prompts.
- MCP prompt discovery/get plus REPL `/mcp/<server>/<prompt>` and `/mcp__server__prompt` slash commands.
- `doctor` capability reporting and `exec --image` path validation plus native OpenAI/Anthropic image payload construction.
- REPL `/compact` summarizes older turns into one transcript summary turn and triggers `pre_compact` first.
- `deepseek update --check/--print-command` for source-checkout update workflows.
- `deepseek update package` for local release package generation.
- `deepseek update install-package` and `deepseek update rollback` for local binary upgrade/rollback.
- `deepseek update verify-install` for clean install smoke covering version, config init, doctor, exec JSONL, and benchmark sample.

Remaining before `<10%`:

- 100+ live CLI dogfood samples with category success rates.

## Immediate Next Slice

Continue with the remaining `<10%` blockers:

1. Run and record 100+ model-backed live CLI dogfood samples, then require
   `--require-live-runs`, `--require-live-success-rate`, and
   `--require-live-category` gates so offline replay rows cannot satisfy the
   public readiness target. Use `deepseek dogfood live-plan --limit 10` to
   choose the next category-balanced replay batch before spending online model
   calls.
