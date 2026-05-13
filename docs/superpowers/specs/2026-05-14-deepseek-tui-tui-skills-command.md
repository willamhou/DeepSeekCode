# DeepSeek-TUI TUI Skills Command

Status: implemented

## Gap

DeepSeek-TUI exposes `/skills` for local/remote skill inventory and `/skill` for
activating or managing a skill. DeepSeekCode already had TOML-backed repo/user
skills and a model-visible `load_skill` tool, but the TUI command registry did
not expose a workbench surface for listing or inspecting skills.

## Implementation

- Added built-in `skills [prefix]` / `/skills [prefix]` parsing and
  `skill <name>` / `/skill <name>` parsing before custom slash-command
  fallback.
- Added `TuiSkillsCommand`, `TuiAction::Skills`, and
  `TuiMcpDetailKind::Skills`.
- Routed local file-backed TUI skills actions through the existing
  DeepSeekCode TOML skill registry: repo skills plus configured
  `workspace.user_skills_dir`, with user skills overriding repo skills.
- Rendered skill lists, prefix-filtered lists, override notes, searched paths,
  and one-skill detail views with description, allowed tools, triggers,
  suggested steps, references, policy, shell allowlist, and system append.
- Kept remote install/sync/update/uninstall/trust commands explicitly out of
  scope for this slice because DeepSeekCode's skill system is local TOML, not
  DeepSeek-TUI's remote `SKILL.md` registry.
- Updated TUI documentation and the DeepSeek-TUI parity plan.

## Verification

- `cargo test skills --lib`
- `cargo test composer_intercepts_memory_prefix_and_slash_commands --lib`
- `cargo test tui --lib`
- `cargo fmt --check`
- `cargo check`
- `git diff --check`

## Remaining

Remote skill registry sync/install/update/uninstall/trust is still a deliberate
gap. Full activation semantics also differ: DeepSeekCode activates skills
through `exec --skill`, REPL `/skill`, and model-visible `load_skill`, while this
TUI slice focuses on inventory and inspection.
