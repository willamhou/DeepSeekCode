# DeepSeek-TUI Parity: Local TUI Skill Management

## Context

DeepSeek-TUI supports `/skill install`, `/skill update`, `/skill uninstall`,
and `/skill trust`. DeepSeekCode currently lists and shows configured TOML
skills but rejects all skill-management subcommands. Full remote registry
install/update still needs a dedicated downloader and network-policy slice, but
local user-skill management can be made useful now.

## Goals

- Support `/skill uninstall <name>` for skills found in
  `workspace.user_skills_dir`.
- Refuse to uninstall bundled repo skills so TUI commands cannot delete checked
  in project assets.
- Support `/skill trust <name>` by writing a `.trusted` marker beside the
  configured user skill TOML.
- Keep `/skill install` and `/skill update` explicitly unsupported until the
  remote registry/downloader slice lands.
- Update completions, docs, and parity plan.

## Acceptance

- `/skill uninstall pr-review` removes `workspace.user_skills_dir/pr-review.toml`
  and refreshes the skill detail panel.
- `/skill trust pr-review` writes
  `workspace.user_skills_dir/pr-review.trusted` and reports the marker path.
- Missing skills and bundled-only skills return actionable status messages.
- Existing `/skills`, `/skill <name>`, and direct `/<skill-name>` behavior
  remains unchanged.
- Full `tui` tests continue passing.
