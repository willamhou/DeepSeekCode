# Phase 12D Skills And Custom Command Validation

Date: 2026-05-23

## Goal

Close the Phase 12D skills/custom command gap with a local, user-runnable gate
that proves skill discovery and metadata are healthy before runtime use.

## Scope

- `deepseek skills list [--json] [--dir <path>]...`
- `deepseek skills validate [--json] [--strict] [--dir <path>]...`
- Discovery docs for skill and custom slash command locations.
- Example skill shapes for PR review, release-check, and security-lite.

Custom slash command execution already existed through `.dscode/commands/*.md`
and `workspace.user_commands_dir`; this spec adds the companion CLI surface for
skill discovery and validation.

## Runtime Compatibility

The validator uses the same directory precedence as agent runtime skill loading:

1. Bundled repo or install-adjacent `skills/`
2. Configured `workspace.user_skills_dir`

Later directories still override earlier skill names. Overrides are reported as
structured metadata, not treated as warnings, because user-level overrides are
an intentional extension mechanism.

The validator deliberately does not make the hand-rolled TOML loader stricter.
That preserves existing runtime compatibility while giving users a stricter
preflight gate when they opt into `skills validate --strict`.

## Validation Rules

Per `.toml` skill file:

- loader errors are errors
- empty `name`, `description`, `system_append`, or `suggested_steps` are warnings
- empty `allowed_tools` is a warning because it grants access to all tools
- unknown `allowed_tools` entries are warnings
- dynamic MCP tool names beginning with `mcp__` are accepted

Directory paths that exist but are not directories are errors. Missing
directories are reported as skipped, matching runtime behavior.

## JSON Shape

`deepseek skills validate --json` emits:

```json
{
  "kind": "deepseek.skills_validate.v1",
  "strict": true,
  "ok": true,
  "total_files": 16,
  "valid_files": 16,
  "error_count": 0,
  "warning_count": 0,
  "dirs": [],
  "entries": [],
  "overrides": []
}
```

`deepseek skills list --json` uses the same report shape with
`kind = "deepseek.skills_list.v1"`.

## Acceptance Evidence

Local commands run on 2026-05-23:

```bash
cargo test cli::commands::skills --lib
cargo test cli::app::tests::parses_skills_subcommands --lib
cargo test cli::commands::help::tests::skills_help_documents_validation_gate --lib
cargo run --quiet -- skills validate --json --dir skills
cargo run --quiet -- skills list --json --dir skills
cargo run --quiet -- skills validate --strict --json --dir skills
```

Observed bundled skill result:

- `total_files = 16`
- `valid_files = 16`
- `error_count = 0`
- `warning_count = 0`
- `ok = true`

## Remaining Phase 12D Work

- Subagent gates: explicit parallel request parsing, write-set nudges,
  parent readback for child-edited files, and conflict/blocker summaries.
