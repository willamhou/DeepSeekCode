# DeepSeek-TUI Parity: TUI Skill ZIP Archive Installer

## Context

DeepSeekCode already imports direct TOML skills, direct `SKILL.md` URLs, GitHub
repository tarballs, and direct `.tar.gz` / `.tgz` archives into the local TOML
skill format. DeepSeek-TUI-style community skill bundles can also be distributed
as zip archives, so rejecting `.zip` sources leaves one narrow installer parity
gap.

## Goals

- Support direct `.zip` skill sources and registry entries whose `source`
  resolves to a zip archive.
- Read zip archives in memory and convert the best safe `SKILL.md` candidate
  into DeepSeekCode TOML with the existing `SKILL.md` importer.
- Apply the existing remote skill download size limit to total uncompressed zip
  entry size.
- Reject unsafe zip paths, malformed zip archives, missing `SKILL.md`, and
  invalid UTF-8 / frontmatter with actionable detail-panel messages.
- Preserve existing install/update/sync metadata and behavior for TOML,
  `SKILL.md`, GitHub, and tar.gz sources.

## Acceptance

- `/skill install <zip-url>` writes `<name>.toml` and an `.installed-from`
  marker when the zip contains a valid `SKILL.md`.
- `/skills sync` caches registry zip entries as TOML files and reports them in
  downloaded/up-to-date/skipped/failed counts.
- Zip entries with parent-directory traversal or absolute paths are rejected.
- Zip archives without a usable `SKILL.md` fail with a clear missing-skill
  message.
- Archive candidate selection prefers root `SKILL.md`, then
  `skills/<name>/SKILL.md`, then one-level nested `SKILL.md`, matching the
  tar.gz importer.
- Full `tui` tests continue passing.
