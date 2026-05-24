# Dogfood Evidence

This document keeps the longer evidence commands out of the README while
preserving the release-readiness path.

## Scope

Dogfood evidence is used to prove that DeepSeekCode can do real code-agent work:
edit files, validate the result, record the run, and fail closed when evidence
does not match the ledger.

The most useful public-beta evidence today is:

- online multi-file external fixture evidence;
- live dogfood report gates;
- release-binary smoke checks through `deepseek update release-smoke`.

## External Write Fixture

Use a disposable git repository outside this checkout. The command dry-runs
preflight first, then runs against an isolated copy and records the result in
the dogfood report.

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

## Live Dogfood Evidence

Plan and inspect live cases before spending online model calls:

```bash
deepseek dogfood report --limit 10
deepseek dogfood live-plan --limit 10
deepseek dogfood live-run --limit 3
deepseek dogfood live-run --limit 3 --json
```

When you intend to run online model-backed cases, provide the API key through a
temporary file outside the repository:

```bash
printf '%s\n' '<deepseek-api-key>' > /tmp/deepseek-live.key
chmod 600 /tmp/deepseek-live.key
deepseek dogfood live-run --api-key-file /tmp/deepseek-live.key \
  --limit 3 \
  --evidence-out .dscode/dogfood/live-evidence.json \
  --execute
deepseek dogfood live-evidence --file .dscode/dogfood/live-evidence.json \
  --out .dscode/dogfood/live-evidence-verification.json \
  --require-benchmark-gate --require-report-gate
rm -f /tmp/deepseek-live.key
```

`live-evidence --require-report-gate` verifies the structured gate, rechecks the
ledger fingerprint from the evidence file, and matches appended case evidence
back to current ledger rows.

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
  --require-live-category pr_workflow:25:90
```

## Release Binary Smoke

After installing a published release binary, verify the current platform with:

```bash
deepseek update release-smoke --version 0.1.1 --json
```

This is the lightweight operator command for checking release archive
availability, checksum coverage, extraction, `deepseek version`, and the local
smoke path supported by the installed platform.

## Related Docs

- [Release checklist](./release.md)
- [Current status](./current-status.md)
- [Public beta guide](./public-beta.md)
