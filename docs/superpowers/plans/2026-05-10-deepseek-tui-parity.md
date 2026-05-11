# DeepSeek-TUI Parity Plan

**Status:** active
**Source comparison:** `Hmbown/DeepSeek-TUI` cloned to `/tmp/deepseek-tui-compare-20260510`, HEAD `506343f`.
**Current repo:** `willamhou/DeepseekCode` (`PRIVATE` at audit time), release command `deepseek`, compatibility alias `dscode`.

## Objective

Move DeepseekCode from a regression-gated CLI code agent toward a full terminal workbench comparable to DeepSeek-TUI, while preserving the existing benchmark/dogfood/trend/live gates.

## Baseline Gap

DeepSeek-TUI is a 14-crate Rust workspace with a dedicated `deepseek` dispatcher, `deepseek-tui` runtime, TUI state machine, HTTP/SSE runtime API, MCP server mode, SQLite-backed durable state, LSP diagnostics, keybindings, package wrappers, and release automation. Its Rust source under `crates/` is about `196k` lines.

DeepseekCode is currently a mostly single-crate CLI with a strong deterministic test/benchmark/dogfood surface and about `36.8k` lines under `src/`. Its CLI core is relatively close, but the terminal product surface is still incomplete.

## Deliverables

1. True TUI
   - Add a `ratatui`/`crossterm` UI behind a stable command path.
   - Support Plan / Agent / YOLO modes.
   - Add approval modal, command palette, transcript scrolling, sidebar, and session picker.

2. Durable Runtime
   - Add durable thread/session records, resume/fork, event timeline, crash checkpoint, task queue, and job center.
   - Target SQLite for durable state once dependency and release strategy are explicit.

3. Tool Surface Expansion
   - Add `web_search`, `fetch_url`, git history tools, background shell wait/interact/cancel, large-output retrieval, test runner, structured data validation, project map, turn revert, and guarded GitHub write tools.

4. DeepSeek-Native UX
   - Add auto model routing, reasoning effort tiers, token/cost status, prefix-cache reporting, and long-context management.

5. LSP Diagnostics
   - Run language server diagnostics after write/edit/patch operations and inject actionable diagnostics into the next model turn.

6. Subagent / RLM
   - Expand beyond current subagent dispatch into role-aware spawn/wait/send/resume/cancel/list/assign flows.
   - Add cheap flash fan-out / RLM-style one-shot child analysis.

7. MCP And Runtime API
   - Add local supervisor contracts: `doctor --json`, `serve --http`, `serve --mcp`, and later `serve --acp`.
   - Surface sessions, threads, turns, events, tasks, automations, usage, skills, and MCP introspection.

8. Packaging
   - Add npm wrapper, Cargo package strategy, Homebrew/Docker artifacts, cross-platform release assets, version sync, and stronger release checklist.

## Current Increments

The first low-risk foundation is `deepseek doctor --json`.

Acceptance:
- `deepseek doctor --json` emits valid JSON.
- JSON includes version, workspace, model, capabilities, API key status without secret leakage, skills, MCP, network probe state, and local binary availability.
- JSON mode does not perform live network probes.
- Release docs require `doctor --json` output as an artifact.

The second low-risk tool-surface increment is read-only Git history:

Acceptance:
- `git_log`, `git_show`, and `git_blame` are first-class agent tools.
- Tool schemas are exposed to OpenAI/Anthropic-compatible tool calling.
- The offline planner routes direct Git history/blame requests without falling back to shell.
- Default benchmark coverage includes all three tools.

## Phase Order

### Phase A: Integration Contract

- `doctor --json`
- `serve --http` skeleton with health endpoint
- thread/session schema draft
- public-readiness checklist

Status: `done locally`

Artifacts:

- `src/cli/commands/doctor.rs`
- `src/cli/commands/serve.rs`
- `docs/runtime.md`
- `docs/release.md`
- `docs/install.md`

### Phase B: Durable State

- Thread/session/event model
- Resume/fork over durable records
- Crash checkpoint
- Background job metadata

Status: `started`

Landed first slice:

- `src/core/runtime.rs` file-backed session/thread/turn/item/event/task/automation/usage store under `.dscode/runtime/`
- `GET /v1/automations`, `POST /v1/automations`, `GET /v1/automations/{id}`
- `GET /v1/sessions`, `POST /v1/sessions`, `GET /v1/sessions/{id}`
- `GET /v1/sessions/{id}/automations`, `POST /v1/sessions/{id}/automations`
- `POST /v1/sessions/{id}/threads`
- `GET /v1/sessions/{id}/tasks`, `POST /v1/sessions/{id}/tasks`
- `GET /v1/tasks`, `POST /v1/tasks`, `GET /v1/tasks/{id}`
- `GET /v1/threads`, `POST /v1/threads`, `GET /v1/threads/{id}`
- `GET /v1/threads/{id}/automations`, `POST /v1/threads/{id}/automations`
- `POST /v1/threads/{id}/turns`
- `GET /v1/threads/{id}/items`, `POST /v1/threads/{id}/items`, `GET /v1/threads/{id}/items/{item_id}`
- `GET /v1/threads/{id}/turns/{turn_id}/items`, `POST /v1/threads/{id}/turns/{turn_id}/items`
- `GET /v1/threads/{id}/tasks`, `POST /v1/threads/{id}/tasks`
- `GET /v1/threads/{id}/events?since_seq=N`
- `GET /v1/threads/{id}/events/stream?since_seq=N&wait_ms=M` for SSE replay plus bounded live wait frames
- `GET /v1/threads/{id}/events/stream?since_seq=N&follow=1` for long-lived SSE follow streams that emit multiple runtime events on one connection until disconnect
- non-`--once` HTTP runtime accepts connections concurrently, so bounded/following SSE streams do not block concurrent runtime writes
- `POST /v1/threads/{id}/events` for appending durable `permission_request` and `permission_response` events consumed by the TUI approval modal
- `GET /v1/threads/{id}/usage`, `GET /v1/usage?thread_id={id}`
- `GET /v1/threads/{id}/usage/summary`, `GET /v1/usage/summary?thread_id={id}` for aggregate token accounting, cache telemetry, recognized DeepSeek V4 cost estimates, and 1M-context policy
- Successful `deepseek exec` runs now append durable sessions, linked user/assistant turns, matching message items, completed task records, and token/cache/cost usage records
- `/runtime` now advertises `sessions`, `threads`, `turns`, `items`, `events`, `events_write`, `events_sse`, `events_sse_wait`, `events_sse_follow`, `tasks`, `automations`, `usage`, and `usage_summary` as available; `deepseek tui --runtime-url http://HOST:PORT` can build snapshots from the HTTP runtime, write foreground actions back over HTTP, and subscribe to known thread event streams with `follow=1`

### Phase C: Tool Surface

- Background shell manager
- Web search/fetch
- Git history (`git_log`, `git_show`, `git_blame` landed as read-only tools)
- Large-output retrieval
- Test/validate/project-map tools

### Phase D: TUI

- Ratatui app shell
- Transcript/composer/status
- Mode switching
- Approval modal
- Command palette/sidebar/session picker

Status: `started`

Landed first slice:

- `Cargo.toml` now includes `ratatui` and `crossterm`
- `src/tui.rs` implements a full-screen ratatui/crossterm TUI shell
- `deepseek tui`, `deepseek tui --demo`, and `deepseek tui --demo --once`
- Plan / Agent / YOLO mode tabs
- sidebar, transcript/composer frame, task panel, command bar
- command palette, session picker, thread navigator, and approval modal surfaces
- session picker reads file-backed durable session metadata from `.dscode/runtime/sessions`
- TUI startup preloads linked runtime threads and item timelines, and the session picker plus thread navigator switch the visible durable transcript snapshot
- interactive TUI refreshes file-backed runtime sessions, threads, and item timelines while open; `--once` remains deterministic
- TUI refresh also reads durable `permission_request` events and opens the approval modal with real tool/kind/target details
- composer focus/input can append user turns and message items to the active durable runtime thread
- approval accept/deny writes durable `permission_response` events and answered requests no longer reopen after refresh
- interactive composer submissions now start a background agent run for the active durable thread
- background TUI agent runs create a running assistant message item, stream assistant deltas into it through durable item updates, and then write final assistant messages, tool result items, usage records, and completed/failed task records back into runtime
- TUI-started agent runs also send assistant/reasoning item updates through an in-process live event channel drained before each draw, so visible token streaming is no longer tied to the 1s durable refresh interval
- interactive TUI starts a local runtime watcher that detects external durable runtime writes and sends full snapshot live events into the draw loop for faster item/task/approval/usage visibility
- TUI-started agent runs use a runtime-backed approval resolver: permissioned write/shell/MCP tool calls append durable `permission_request` events, wait for the modal's `permission_response`, and then continue approved calls or record denied tool observations
- `deepseek agents run-task` and daemon-executed tasks also append durable permission requests and wait for matching thread `permission_response` events, so external TUI/HTTP clients can approve background tasks
- TUI-started agent runs now create a running runtime task, expose `c` / `cancel` for the active running assistant turn, write a durable `cancel_requested` event, and mark the turn/item/task `cancelled` at cooperative checkpoints
- TUI task panel now loads active-thread runtime task records and shows kind/status/summary progress for recent background work
- TUI command palette can create pending active-thread `agent` tasks with `task <summary>` / `task create <summary>`, so daemon or external runners can pick up new work from the workbench
- TUI task panel now loads active-thread automations, and the command palette can trigger current-thread automations into pending runtime tasks with optional prompt overrides
- task panel now surfaces active thread usage totals, cache-hit rate, cache chart, estimated cost, input/output cost split, cost chart, and 1M-context policy from durable usage records
- command palette executes local UI commands for mode switching, session picker, thread navigator, thread next/prev/id switching, and approval modal, plus runtime mutations for active-thread `task <summary>`, `compact [tail]`, `automation trigger [id]`, cancel, approval response, and composer submit
- AgentLoop cancellation now propagates into cancel-aware model/tool execution; `run_shell` starts commands in a process group and kills that group when a durable cancel event is observed, while remote model streams stop between SSE frames
- deterministic `--once` snapshot path for CI/release smoke tests

Remaining:

- true cross-process push/SSE subscription into the foreground TUI beyond the local runtime watcher
- fully interrupting blocked model socket reads, plus richer progress controls for background TUI agent runs
- command palette actions executing arbitrary external commands
- scrollback, composer editing, and richer keyboard model

### Phase E: DeepSeek-Native Product UX

- Auto model router
- Reasoning tiers
- Cost/cache telemetry
- 1M-context compaction controls

Status: `started`

Landed first slice:

- `model.model = "auto"` and `DEEPSEEK_MODEL=auto` now route simple work to `deepseek-v4-flash` and complex planning/review/architecture/security/migration/recovery work to `deepseek-v4-pro`; `model.reasoning_effort = "auto"` maps the same route to off/high/max thinking tiers
- remote usage records attach the resolved model name so Runtime cost accounting records `deepseek-v4-flash` / `deepseek-v4-pro` instead of an opaque `auto`
- model usage parsing now preserves OpenAI-compatible `prompt_cache_hit_tokens` / `prompt_cache_miss_tokens`, `prompt_tokens_details.cached_tokens`, and Anthropic-compatible cache read/creation counters when providers return them
- runtime usage records persist prompt cache hit/miss tokens and recognized DeepSeek V4 USD micro-cost estimates
- usage summary aggregates cache hit rate, estimated input/output/total cost, unpriced record count, and 1M-context strategy
- TUI usage panel renders cache and cost split bars so DeepSeek prefix-cache and cost behavior is visible during durable sessions
- TUI command palette can trigger non-destructive active-thread compaction with `compact [tail]`, reusing the runtime `thread_compacted` audit event path
- `model.reasoning_effort = "off|high|max|auto"` and `DEEPSEEK_REASONING_EFFORT` now map to official DeepSeek V4 thinking/reasoning parameters for OpenAI-compatible and Anthropic-compatible requests; streaming parsers surface reasoning deltas separately from final answer text

Remaining:

- richer reasoning UX, including full reasoning-content replay for multi-turn tool-call conversations
- model-generated automatic compaction policy

### Phase F: LSP + Revert

- Language server registry
- Post-edit diagnostics
- Turn snapshots and `revert_turn`

Status: `started`

Landed first slice:

- `src/language/diagnostics.rs` adds a diagnostics runner that prefers stdio LSP `textDocument/publishDiagnostics` for opened files when the language server is available, then falls back to compiler/type-check commands
- `deepseek diagnostics [--changed] [paths...]` exposes manual diagnostics for Rust, TypeScript, JavaScript, Python, and Go workspaces
- `deepseek diagnostics --watch` keeps a warmed stdio LSP session alive inside the watcher process, and `deepseek agents service` renders a diagnostics watch supervisor for local always-on use
- agent registry exposes a read-only `diagnostics` tool, and OpenAI/Anthropic tool schemas include it
- `diagnostics.post_edit = true` enables opt-in post-edit diagnostics appended to successful `apply_patch` tool results
- `src/core/rollback.rs` stores rollback snapshots under `.dscode/rollback/snapshots/`, including combined, staged, and unstaged tracked diffs plus captured untracked regular files
- `deepseek restore snapshot [label]`, `restore list`, `restore show <id> [--patch]`, and `restore revert-turn <id> [--apply]`
- REPL `/restore snapshot [label]`, `/restore list`, `/restore show <id>`, and `/revert_turn <id> [--apply]`
- Snapshot restore checks that git `HEAD` matches the captured commit, dry-runs by default, and applies only when `--apply` is passed
- Applied restores now report restored changed files and run post-restore diagnostics through the same fallback diagnostic runner
- Applied restores now restore captured untracked regular files and exclude rollback storage from untracked capture
- Applied restores preserve the snapshot staged-index versus unstaged-worktree split for new split-patch snapshots
- `deepseek exec` creates a pre-run rollback snapshot in git worktrees and binds it to the successful assistant runtime turn id; restore/show accept either snapshot id or bound turn id
- TUI-started agent runs create a pre-run rollback snapshot in git worktrees and bind it to the running assistant turn id as soon as the durable turn exists

Remaining:

- cross-process diagnostics broker shared by TUI, daemon, and CLI processes
- automatic turn snapshots for REPL live turns
- side-git/worktree snapshot strategy for richer non-regular-file fidelity
- richer restore UX in the future TUI

### Phase G: Subagent/RLM

- Role-aware child agents
- Child lifecycle controls
- Flash fan-out RLM helper

### Phase H: Packaging

- npm wrapper
- release artifact matrix
- Docker/Homebrew plan
- version sync and publish dry-run checks

Status: `started`

Landed first slice:

- `Dockerfile` and `.dockerignore` for source-built local Docker images
- `npm/package.json`, `npm/bin/deepseek.js`, and `npm/README.md` for a Node wrapper that launches packaged target-triple binaries or `DEEPSEEK_BINARY`
- `docs/install.md` and `docs/release.md` include Docker and npm wrapper verification commands
- `Cargo.toml` now carries publish metadata (`description`, `readme`, `license-file`, repository/homepage, keywords, categories)
- `.github/workflows/release.yml` defines a release matrix for Linux x64, macOS x64, macOS arm64, and Windows x64, plus packaging checks for Cargo metadata, npm wrapper, npm dry-pack, and Homebrew formula syntax
- Release matrix archives now include sibling `.sha256` files for published asset verification and Homebrew formula updates
- Release matrix creates GitHub signed artifact attestations for each archive and checksum file with `actions/attest`
- `packaging/homebrew/deepseek.rb` provides a Homebrew formula template for macOS arm64/x64 and Linux x64 release assets
- `packaging/systemd/` and `packaging/launchd/` provide runtime service placeholders; `deepseek agents service` renders workspace-specific systemd/launchd files for `serve --http`, `agents daemon --json`, and `diagnostics --watch --changed`
- `deepseek update package` includes `SERVICES.md` and packaged service templates under `services/`

Remaining:

- Actual published npm package with uploaded platform binaries
- Actual Cargo publish or explicit private registry release decision
- Published Homebrew tap with real release asset SHA-256 values

## Completion Audit Gate

This plan is complete only when every deliverable has:
- code merged into a tracked source path,
- docs that explain user-facing usage,
- focused unit/integration tests,
- inclusion in release or benchmark/dogfood verification where applicable,
- a source-level comparison note explaining remaining differences from DeepSeek-TUI.
