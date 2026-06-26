# Hermes Compatibility Spike

A one-page, time-boxed experiment to settle a strategic question before any
code commitment: **should DeepSeekCode and Hermes Agent RS share runtime
code, build a thin compatibility membrane between them, or stay strictly
separate with cross-linked READMEs?**

Date drafted: 2026-06-26. Revised the same day after independent review.
Owner: TBD. Timebox: 1 calendar week of focused work.

## Background

- **DeepSeekCode** — DeepSeek-first terminal code-agent CLI in Rust, deliberate
  minimal-deps stance (no serde / no async runtime / no HTTP client; JSON, CLI,
  and HTTP (via `curl`) are hand-rolled).
- **Hermes Agent RS** — horizontal Rust agent framework, 11-crate workspace,
  multi-provider, tokio/serde-driven, OpenAI-compatible gateway + Telegram +
  managed-agents control plane.

Both are owned by the same operator. The architectural question is whether to
DRY them. Extracting a shared runtime crate has been rejected as premature
abstraction: the dependency stances are incompatible, three-repo coordination
cost is high, and Rig / Swiftide / Goose already occupy the generic-runtime
lane. The remaining live question is whether a **compatibility membrane** can
exist — protocol shapes and external adapters, not shared internals.

## The single question this spike answers

> Can Hermes **inspect** (read-only: list + render) a DeepSeekCode tool
> transcript — without compromising any of DeepSeekCode's minimal-deps
> invariants and with every lossy field documented up front?

We deliberately split **inspect** from **resume**. Inspect can survive a lossy
transcript; resume is a much more expensive semantic contract (provider-
normalized messages, tool IDs, status transitions, agent state). If the
inspect-only first spike turns RED, resume was never going to be cheap. If it
turns GREEN, resume is a follow-up spike (see end of this doc) — not a
ride-along.

Pick the **smallest** exchange between the two projects (read-only tool-call
plus observation events), not the full surface (provider abstraction, MCP
routing, gateway, session resume). If the smallest one won't fit, the larger
ones definitely won't.

## Day 1–2: paper semantic mapping (no code)

The first 1–2 days produce ONE document, not a single line of code. Take a
real `.dscode/runtime/events/<thread>.jsonl` — the calc bugfix fixture
(reproducible with `docs/demo/record-calc-bugfix-exec.sh`) is the canonical
input. Map every recorded field to exactly one of:

- **carry verbatim** — Hermes can store the exact value with no
  interpretation,
- **carry as opaque metadata** — Hermes treats it as a string blob, never
  acts on it,
- **drop with documented loss** — the protocol spec records this category
  explicitly,
- **fabricate** — Hermes would need to invent something not in the source
  stream.

If **any** field needed for the inspect target (list + render of tool calls
and observations) lands in **fabricate**, the spike is RED on day 2 and the
remaining 5 days are not spent. No code is written until this mapping is
complete and contains zero "fabricate" entries for inspect-scope fields.
This gate is the one that earns the spike — it pays for itself even when it
fires.

## Concrete deliverables (in order, gated by Day 1–2)

1. **Protocol spec** — `agent-event-protocol.md` in a tiny stand-alone repo,
   covering ONLY `tool_call` and `observation` event types for v0.1. Model
   routes, repair events, usage events, and any resume-required fields are
   explicitly deferred to v0.2 / a later spike.
2. **DeepSeekCode emitter** — one new subcommand, `deepseek events export
   --format agent-event-protocol <thread>`, built on the existing hand-rolled
   `util::json` helpers. **Zero new dependencies.**
3. **Hermes read-only transcript reader** — list imported transcripts and
   render the tool-call + observation timeline in Hermes's UI. **No resume
   API.** Imported transcripts must be visibly marked as imported (not
   native) so users cannot mistake them for resumable sessions.
4. **End-to-end test** — DeepSeekCode run → `events export` → Hermes imports
   → Hermes lists it and renders the timeline. Run on the calc-bugfix
   fixture.

## Success criteria (all must hold)

| Metric | Target | Pass condition |
| --- | --- | --- |
| New dependencies on DeepSeekCode | **0** | `Cargo.toml` deps list unchanged; no serde / no tokio / no http crate introduced |
| Lines touched (both repos, excl. tests + docs) | **< 300** | Combined diff stat under 300 LOC |
| Release coupling | **independent** | Protocol versioned in its own repo; neither project hard-pins the other's version |
| DeepSeekCode invariants | **all preserved** | Audit: no serde, no async runtime, no HTTP crate, HTTP still via `curl`, CLI / JSON still hand-rolled |
| Data fidelity | **no fabrication** | Day 1–2 mapping shows zero "fabricate" entries for inspect-scope fields; "drop with documented loss" entries are enumerated in the protocol spec |
| Unknown-event tolerance | **forward-compatible** | Hermes reader does not crash or silently corrupt when it encounters an unknown event type or unknown field |
| Imported vs native separation | **visible** | Hermes UI and store cannot silently treat an imported transcript as a native session (no accidental resume) |

## Out of scope (explicit; do not let scope creep in)

- No shared `Tool` / `Provider` / `Session` traits between projects.
- No runtime crate extraction.
- No async runtime in DeepSeekCode.
- No TUI work in Hermes.
- No bidirectional adapter (Hermes → DeepSeekCode); start one-way.
- **No resume of imported transcripts in Hermes.** Resume is a separate,
  later spike.
- **No model routes, repair events, or usage events** in protocol v0.1.

## Decision after the spike

| Outcome | Meaning | Next step |
| --- | --- | --- |
| **GREEN** — all metrics pass within timebox | Read-only transcript membrane works | Plan the resume spike (below). Quantify what Hermes would need to add to its session model to round-trip a DeepSeekCode transcript without fabrication. |
| **YELLOW** — works but one metric overshoots | Pattern is right, scope is wrong | Narrow further (e.g., transcript metadata only, or render only one event type) and repeat. |
| **RED** — Day 1–2 mapping shows fabrication, or an invariant breaks, or the timebox blows up | Membrane is more expensive than separation, even read-only | Stop. Land cross-linked READMEs ("see Hermes for X") and do not revisit until provider/model conditions change. |

The whole point of the spike is to make the right answer **measurable**, not
debated. If the result is RED on day 2, you save a week. If it is GREEN, you
have one real protocol level to build on — not a session-resume claim that
cannot be honored.

## Second spike (deferred): resume fidelity

If the inspect spike turns GREEN, the next spike — separately, also one-week
timeboxed — asks: **what would Hermes need to add to its session model to
resume a DeepSeekCode transcript without fabricating provider-normalized
messages, tool IDs, or status transitions?** That spike will probably surface
either (a) a small additive set of fields that both projects already record
under different names — cheap to bridge — or (b) a fundamental mismatch
(e.g. Hermes expects provider-side tool IDs that DeepSeekCode never assigns)
that makes resume permanently expensive. Knowing which one is the strategic
question the resume spike answers; the inspect spike answers nothing about
resume by design.

## Quick checklist to start

- [ ] **Day 1–2 (no code)** — semantic field mapping on the calc-bugfix
      fixture. If any inspect-scope field lands in "fabricate", stop here:
      RED.
- [ ] Pick the smallest event subset to cover first: `tool_call` +
      `observation` only.
- [ ] Spin an `agent-event-protocol` repo with just the spec + a JSON Schema.
      Document the "drop with documented loss" categories from Day 1–2
      explicitly.
- [ ] Sketch the DeepSeekCode emitter against `util::json` — verify zero new
      deps before writing the implementation.
- [ ] Sketch the Hermes read-only reader path against `hermes-config` session
      store, with imported-vs-native separation visible in the UI.
- [ ] Run the end-to-end test on the calc-bugfix fixture (the agent take is
      reliable).
- [ ] At end of week: write a one-paragraph result with each metric, then
      decide GREEN / YELLOW / RED.
