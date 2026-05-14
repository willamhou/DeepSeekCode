# DeepSeek-TUI Parity: TUI Skill Registry Cache Sync

## Context

DeepSeek-TUI supports `/skills sync` to fetch the configured community registry
and cache every listed skill locally without installing it into the active skill
directory. DeepSeekCode now supports remote registry browsing and installing
direct TOML skill sources, but `/skills sync` still returns unsupported.

DeepSeekCode does not yet support GitHub/tarball/SKILL.md archive extraction,
so this sync slice caches only registry entries whose source is a direct TOML
URL. Archive entries are reported as skipped and remain part of the later
archive-installer slice.

## Goals

- Add configurable `skills.cache_dir` for remote skill cache files.
- Support `/skills sync` and `/skills --sync` from the TUI composer and command
  palette.
- Fetch `skills.registry_url`, download supported TOML skill sources, validate
  each TOML skill, and write them under `skills.cache_dir`.
- Write per-skill cache metadata with source URL and checksum.
- Report downloaded, up-to-date, skipped, and failed counts in the detail
  panel.

## Acceptance

- `/skills sync` downloads supported TOML registry entries into
  `skills.cache_dir`.
- Re-running sync against unchanged content reports the entry as up-to-date.
- Unsupported GitHub/tarball entries are skipped with an actionable reason.
- Existing `/skills [prefix]`, `/skills --remote`, `/skill install`,
  `/skill update`, `/skill <name>`, `/skill new`, `/skill trust`,
  `/skill uninstall`, and direct `/<skill-name>` behavior remains unchanged.
- Full `tui` tests continue passing.
