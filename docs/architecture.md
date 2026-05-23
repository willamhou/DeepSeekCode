# Architecture

DeepSeekCode is a terminal-first code-agent CLI. The implementation is organized
around a local agent loop, a durable runtime store, permissioned tools, and
multiple human/operator surfaces over the same core behavior.

## Layers

### CLI And UI

The public entrypoint is `deepseek`.

- Bare `deepseek` opens the full-screen TUI when stdin/stdout are real TTYs.
- `deepseek chat` opens the line-oriented REPL.
- `deepseek run` / `deepseek exec` run one-shot or scriptable tasks.
- `deepseek agents`, `deepseek task`, `deepseek github`, `deepseek mcp`,
  `deepseek dogfood`, and `deepseek update` expose operational surfaces for
  runtime workers, CI, release, and evidence.

The TUI and REPL are separate frontends over the same agent/runtime/tooling
contracts. The TUI emphasizes durable sessions, approvals, task panels, MCP
manager screens, and live runtime refresh. The REPL emphasizes fast local prompt
iteration, slash commands, raw-mode line editing, and session save/load.

### Agent Loop

The agent loop builds a `ModelRequest` from:

- user task text;
- workspace instructions (`AGENTS.md`, compatible Claude instruction files, and
  optional user instructions);
- selected profile/model/provider settings;
- recent transcript/runtime context;
- tool observations and recovery hints;
- skill/custom command prompt additions.

The model returns either a final response or a tool call. Tool calls are routed
through the same permission, policy, hook, execution, observation, and recovery
paths regardless of whether the request came from TUI, REPL, exec, runtime
daemon, GitHub Action bridge, or dogfood.

### Model Adapter

The model layer normalizes DeepSeek/OpenAI-compatible and Anthropic-compatible
responses into the same internal action types.

Supported behavior includes:

- OpenAI-compatible function/tool calls;
- same-turn batch tool calls;
- Anthropic-style `tool_use` content blocks;
- streaming assistant deltas and reasoning/thinking deltas;
- provider/model aliases, including DeepSeek V4 aliases and `auto` routing.

The default provider is DeepSeek-first, but the rest of the runtime is not tied
to one provider-specific response shape.

### Tools And Policy

Tools are small, auditable operations behind a registry. Core tools include:

- file and repository inspection (`list_files`, `read_file`, `search_text`,
  `git_diff`, project map/status helpers);
- edits (`apply_patch`, write/move/copy/delete helpers where enabled);
- shell execution and shell-supervisor control;
- diagnostics, tests, review helpers, todo/plan state, notes, memory, rollback;
- MCP/ACP bridge tools;
- runtime task, automation, subagent, and user-input tools;
- web/search/finance/image/document helper tools where configured.

Before side effects run, policy and approval checks decide whether the tool may
execute. Hooks can add context, deny actions, or inject shell environment keys.
Tool results are summarized into observations and persisted into runtime records
when a durable thread is active.

### Durable Runtime

The runtime store lives under `.dscode/runtime/` and records:

- sessions and linked threads;
- turns and item timelines;
- usage and cost/cache telemetry;
- events such as permission requests, approvals, cancellations, and user-input
  requests;
- task and automation records.

The same store is exposed through:

- local file-backed TUI access;
- `deepseek serve --http` REST/SSE endpoints;
- `deepseek agents daemon` and `deepseek agents run-task`;
- local service templates for systemd/launchd;
- release and smoke checks.

Runtime events use append-only records so UI clients and background workers can
coordinate without sharing process memory.

### Shell Supervisor

The shell-supervisor is the long-running shell control plane. It supports:

- safe background shell jobs;
- start/wait/attach/replay/stdin/resize/cancel;
- Linux native PTY jobs;
- byte-stream and raw-proxy modes;
- Linux `pty_fd` handoff for direct PTY master control;
- Windows ConPTY/TCP smoke paths;
- service smoke and installed-service probes.

This lets the agent keep terminal work observable, cancellable, replayable, and
separate from the parent CLI process.

### MCP And ACP

DeepSeekCode can act as:

- an MCP server exposing workspace/runtime tools, resources, and prompts;
- an MCP client for configured stdio/HTTP/SSE servers;
- an ACP stdio adapter for runtime sessions and tool events.

Agent-visible MCP calls go through approval and allowlist policy. Remote MCP
tools can be exposed as dynamic `mcp__server__tool` tools when explicitly
enabled.

### Automation And CI

The automation layer includes:

- `deepseek task` background worktree runner;
- `deepseek github action` bridge for review/fix/patch workflows;
- local fixture smoke commands for tasks, GitHub bridge, hooks, MCP, subagents,
  shell supervisor, and services;
- dogfood live/external evidence recorders and verifiers;
- release packaging and public install readiness checks.

The release goal is to make important behavior repeatable through one-command
gates rather than relying on ad hoc manual proof.

## Safety Principles

- Prefer patches over whole-file overwrites.
- Keep side effects permissioned and auditable.
- Keep credentials out of transcripts, evidence, and committed files.
- Bind release evidence to ledger fingerprints and verifier output.
- Treat historical specs as audit records, not current truth.
- Keep Linux/macOS local CLI readiness separate from broader Windows, hosted
  IDE, and publishing hardening.
