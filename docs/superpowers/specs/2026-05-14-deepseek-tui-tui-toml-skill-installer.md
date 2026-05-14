# DeepSeek-TUI Parity: TUI TOML Skill Installer

## Context

DeepSeek-TUI supports `/skill install` and `/skill update` for community skill
bundles. DeepSeekCode now supports local skill listing, direct skill display,
`/skill new`, local trust/uninstall, and remote registry browsing, but still
rejects install/update.

DeepSeekCode's current skill format is a single TOML file, not a SKILL.md
bundle. The smallest useful installer slice is therefore a TOML skill installer
that downloads a TOML skill from either a direct URL or a configured registry
entry whose `source` is a direct URL. GitHub tarball/SKILL.md extraction remains
a later archive-installer slice.

## Goals

- Support `/skill install <registry-name|https://...>` for TOML skill files.
- Resolve registry names through the configured `skills.registry_url`.
- Write installed skills to `workspace.user_skills_dir/<skill-name>.toml`.
- Write `workspace.user_skills_dir/<skill-name>.installed-from` metadata with
  source and checksum so `/skill update <name>` can refetch it.
- Support `/skill update <name>` for skills installed by this TOML installer.
- Keep GitHub/tarball/SKILL.md sources explicitly unsupported with actionable
  messages.

## Acceptance

- Installing a direct TOML URL writes the TOML skill and `.installed-from`
  marker under the configured user skill directory.
- Installing a registry name fetches the configured registry, resolves its
  source, then installs the TOML skill.
- Updating an installed skill reports no change when the checksum is unchanged
  and replaces the TOML when the source changes.
- Unsupported GitHub/tarball sources produce a clear detail-panel message.
- Existing `/skills`, `/skills --remote`, `/skill <name>`, `/skill new`,
  `/skill trust`, `/skill uninstall`, and direct `/<skill-name>` behavior
  remains unchanged.
- Full `tui` tests continue passing.
