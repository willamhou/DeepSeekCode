# DeepSeek-TUI Parity: TUI Undo and Retry Commands

## Context

DeepSeek-TUI exposes `/undo` and `/retry` as fast recovery commands. `/undo`
removes the latest user request and any following assistant output from the
active conversation. `/retry` removes that same latest exchange and immediately
resubmits the user request. DeepSeekCode already has durable session/thread
records, so the safest parity shape is a non-destructive fork instead of
deleting persisted history.

## Goals

- Add `undo` / `/undo` command-palette and composer support.
- Add `retry` / `/retry` command-palette and composer support.
- Add help detail rendering for `undo help` and `retry help`.
- Fork the selected durable thread up to the turn before the latest user turn,
  keeping the original thread available for audit and navigation.
- Make the fork the selected active thread in the selected session.
- For `/retry`, resubmit the latest user message to the forked thread.
- For `/edit`, when the loaded message is edited and submitted, use the same
  rollback-and-submit path so the replacement does not stack on top of the old
  exchange.
- Reject unsupported arguments with concise usage errors.

## Design

`RuntimeStore` gains a bounded fork operation that copies a source thread,
turns, and turn-linked items through a requested turn count. Items without a
turn are copied only when they occurred before the first dropped turn-linked
item, preventing the removed exchange from leaking into the forked transcript.

`TuiApp` parses `undo` / `/undo` and `retry` / `/retry` into dedicated commands.
The app queues local runtime actions for selected-thread undo, retry, and edited
resubmit. The CLI action handler performs the bounded fork, refreshes the app
snapshot, selects the fork, and optionally starts a new model turn on that fork.

This intentionally leaves the original thread files intact. DeepSeek-TUI mutates
in-memory history; DeepSeekCode preserves prior durable state and moves the
active pointer to the corrected branch.

## Acceptance

- `/undo` creates a new active thread without the latest user turn or following
  assistant turns.
- `/retry` creates the same rollback fork and starts a new turn containing the
  latest user message.
- `/edit` loads the latest user message into the composer, and submitting it
  creates a rollback fork before sending the edited replacement.
- Invoking `/undo` or `/retry` with no previous user turn reports a clear status
  and does not create a thread.
- Help entries, slash completions, and `/help` command listings include
  `/undo` and `/retry`.
- Unit tests cover runtime bounded fork behavior, TUI action queuing, local
  action handling, and edit resubmission.
