# DeepSeek-TUI TUI Guided Setup Actions

**Status:** implemented on 2026-05-14
**Comparison source:** `Hmbown/DeepSeek-TUI` refreshed at `/tmp/deepseek-tui-compare-20260514`, HEAD `9483248a9f35b5f2b56c34b5b84fbc5334473c9d`.

## Gap

The first TUI onboarding slice added a read-only `/setup` checklist. That
narrowed first-run visibility, but still left setup actions scattered across
separate commands. DeepSeek-TUI presents more of the onboarding journey as an
in-terminal flow.

This slice turns `/setup` into a guided command hub while reusing existing
safe TUI controls.

## Implementation

- Add `/setup provider` to open the provider picker.
- Add `/setup model` to open the model picker.
- Add `/setup auth` to focus API key/env guidance without persisting secrets.
- Add `/setup trust` to queue selected-workspace trust inspection.
- Add `/setup theme` to show theme controls.
- Add `/setup language` to show language-output controls.
- Add `/setup settings` to show all focused settings entry points.
- Update help, command completion, slash completion, setup detail text, and
  `docs/tui.md`.

## Verification

- `cargo test setup_subcommands_route_to_guided_controls --lib`
- `cargo test setup_command_renders_onboarding_checklist --lib`
- `cargo test composer_slash_hints_include_deepseek_tui_palette_backed_commands --lib`
- `cargo test composer_slash_hints_cover_deepseek_tui_command_registry_names --lib`
- `cargo fmt --check`
- `git diff --check`

## Residual Gap

This is a guided hub, not a full credential-entry wizard. DeepSeekCode still
does not store raw API keys from the TUI; users provide secrets through
environment variables or existing provider-specific local configuration.
