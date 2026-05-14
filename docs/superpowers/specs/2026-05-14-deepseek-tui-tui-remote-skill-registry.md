# DeepSeek-TUI Parity: Remote Skill Registry Browsing

## Context

DeepSeek-TUI supports `/skills --remote` and `/skills sync` over a curated
community skill registry, plus `/skill install` and `/skill update` download
flows. DeepSeekCode currently handles local TOML skills but reports remote
registry browsing as unsupported.

The full installer needs a larger archive downloader and SKILL.md-to-local
skill import policy. The next useful parity slice is read-only remote registry
browsing so users can discover community skills before installer mutation paths
land.

## Goals

- Add a configurable `[skills] registry_url` with the DeepSeek-TUI default
  registry URL.
- Support `/skills --remote` and `/skills remote` in the TUI composer and
  command palette.
- Fetch the configured registry through the existing network-policy-gated URL
  fetch path.
- Render registry name, description, and source fields in the skill detail
  panel.
- Keep `/skills sync`, `/skill install`, and `/skill update` explicitly
  unsupported until the installer/downloader slice lands.

## Acceptance

- `/skills --remote` fetches a JSON registry with a `skills` object and renders
  sorted remote skill entries.
- Network-policy denial or fetch/parse failures render actionable detail-panel
  messages rather than crashing the TUI action.
- `skills.registry_url` can be set in `.dscode/config.toml`.
- Existing local `/skills [prefix]`, `/skill <name>`, `/skill trust <name>`,
  `/skill uninstall <name>`, and direct `/<skill-name>` behavior remains
  unchanged.
- Full `tui` tests continue passing.
