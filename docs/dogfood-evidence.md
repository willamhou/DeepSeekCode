# Dogfood Evidence

This document keeps the longer evidence commands out of the README while
preserving the release-readiness path.

## Scope

Dogfood evidence is used to prove that DeepSeekCode can do real code-agent work:
edit files, validate the result, record the run, and fail closed when evidence
does not match the ledger.

The most useful public-beta evidence today is:

- tracked online multi-file external fixture evidence for Python, Rust, and
  Node samples;
- deterministic repair/cache evidence for DeepSeek-style malformed tool-call
  recovery and runtime cache diagnostics;
- reusable Python, Rust, and Node external fixture scaffolds;
- live dogfood report gates, including MCP loop-surface coverage;
- release-binary smoke checks through `deepseek update release-smoke`.

## Repair And Cache Evidence

Use this local no-model command to reproduce a formerly failing DeepSeek-style
tool-call trace and prove that the repair and cache diagnostics are visible
through runtime surfaces:

```bash
deepseek dogfood repair-cache-evidence --json
deepseek events replay <after-thread-id> --limit 50
deepseek events diff <before-thread-id> <after-thread-id>
deepseek stats --thread <after-thread-id> --require-prefix-stable
```

The first command writes `.dscode/dogfood/repair-cache-evidence.json`. The JSON
contains the before/after thread ids and command list. Expected evidence:

- strict parsing rejects the truncated `read_file` arguments before repair;
- the after thread records one structured `tool_call_repair` event;
- the after thread records one `prompt_layers_recorded` event;
- `events diff` shows failed tool calls dropping from 1 to 0 and cache hit rate
  increasing from 0% to 75%;
- `stats --thread` shows `repair_count: 1` and prompt-layer diagnostics.

The Release Matrix packaging job runs the same deterministic evidence path with
`--out target/loop-evidence/repair-cache-evidence.json`, gates the resulting
after thread with `deepseek stats --require-prefix-stable --json`, and uploads
the JSON files as the `deepseek-loop-evidence` artifact.

## Fixture Catalog

Use disposable git repositories outside this checkout. The fixture generator
creates a failing repository, commits the starting point, verifies that the
initial validation command fails, and prints dry-run, evidence, and verification
commands.

```bash
base=/tmp/deepseek-external-fixtures
scripts/create-multifile-external-fixture.sh "$base/python-invoice-multifile" python-invoice-multifile --force
scripts/create-multifile-external-fixture.sh "$base/rust-order-multifile" rust-order-multifile --force
scripts/create-multifile-external-fixture.sh "$base/node-task-report" node-task-report --force
```

Available fixture kinds:

- `python-invoice-multifile`: Python package with pricing and invoice label
  edits, validated by `python3 -m unittest discover -s tests`.
- `rust-order-multifile`: Rust crate with pricing and receipt label edits,
  validated by `cargo test`.
- `node-task-report`: Node fixture with archived-task filtering and report
  heading edits, validated by `node tests/report.test.js`.

## External Write Fixture

For release evidence, dry-run preflight first, then run the command against an
isolated copy and record the result in the dogfood report. The Python invoice
fixture remains the canonical tracked sample:

```bash
fixture_dir=/tmp/deepseek-external-fixtures/python-invoice-multifile
scripts/create-multifile-external-fixture.sh "$fixture_dir"
task='replace `return amount - discount` with `return max(amount - discount, 0.0)` in src/invoice_math/pricing.py and replace `Invoice total` with `Final total` in src/invoice_math/summary.py, validate with python3 -m unittest discover -s tests'
deepseek dogfood external-fixture --workdir "$fixture_dir" --dry-run "$task"
deepseek dogfood external-fixture --workdir "$fixture_dir" \
  --evidence-out .dscode/dogfood/external-fixture-python-invoice-multifile-evidence.json \
  "$task"
deepseek dogfood external-evidence \
  --file .dscode/dogfood/external-fixture-python-invoice-multifile-evidence.json \
  --out .dscode/dogfood/external-fixture-python-invoice-multifile-verification.json \
  --require-successful-external-fixtures 1
```

The verification file should report that the external fixture matched the
current ledger and that post-validation passed.

Tracked Rust and Node evidence files:

- `.dscode/dogfood/external-fixture-rust-order-multifile-evidence.json`
- `.dscode/dogfood/external-fixture-rust-order-multifile-verification.json`
- `.dscode/dogfood/external-fixture-node-task-report-evidence.json`
- `.dscode/dogfood/external-fixture-node-task-report-verification.json`

Refresh them with the `evidence_command` and `verify_command` printed by the
generator.

## Live Dogfood Evidence

Plan and inspect live cases before spending online model calls:

```bash
deepseek dogfood report --limit 10
deepseek dogfood live-plan --limit 10
deepseek dogfood live-run --limit 4
deepseek dogfood live-run --limit 4 --json
```

The default live plan targets `write_validate`, `recovery`, `pr_workflow`, and
`mcp`. The MCP slice includes dynamic remote tools, generic `mcp_call`, resource
discovery/readback, and deny-recovery fixtures.

When you intend to run online model-backed cases, provide the API key through a
temporary file outside the repository:

```bash
printf '%s\n' '<deepseek-api-key>' > /tmp/deepseek-live.key
chmod 600 /tmp/deepseek-live.key
deepseek dogfood live-run --api-key-file /tmp/deepseek-live.key \
  --limit 4 \
  --evidence-out .dscode/dogfood/live-evidence.json \
  --execute
deepseek dogfood live-evidence --file .dscode/dogfood/live-evidence.json \
  --out .dscode/dogfood/live-evidence-verification.json \
  --require-benchmark-gate --require-report-gate \
  --require-loop-surface-gate
rm -f /tmp/deepseek-live.key
```

`live-evidence --require-report-gate` verifies the structured gate, rechecks the
ledger fingerprint from the evidence file, and matches appended case evidence
back to current ledger rows. `--require-loop-surface-gate` additionally fails
unless the evidence includes an MCP loop-surface case and the structured
`evidence_gate` requires `mcp` live evidence.

## Release Evidence Gate

For a release-readiness evidence gate, make the report fail closed when the
ledger does not have enough live proof:

```bash
deepseek dogfood report --limit 20 \
  --require-min-runs 100 \
  --require-success-rate 90 \
  --require-live-runs 100 \
  --require-live-success-rate 90 \
  --require-recent-clean 20 \
  --require-external-write-fixtures 3 \
  --require-category write_validate:25:90 \
  --require-category recovery:25:90 \
  --require-category pr_workflow:25:90 \
  --require-live-category write_validate:25:90 \
  --require-live-category recovery:25:90 \
  --require-live-category pr_workflow:25:90 \
  --require-live-category mcp:3:90
```

## Release Binary Smoke

After installing a published release binary, verify the current platform with:

```bash
deepseek update release-smoke --version 0.1.3 --json
```

This is the lightweight operator command for checking release archive
availability, checksum coverage, extraction, `deepseek version`, and the local
smoke path supported by the installed platform.

## Related Docs

- [Release checklist](./release.md)
- [Current status](./current-status.md)
- [Public beta guide](./public-beta.md)
