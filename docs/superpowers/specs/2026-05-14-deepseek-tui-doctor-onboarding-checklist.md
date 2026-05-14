# DeepSeek-TUI Doctor Onboarding Checklist

**Status:** implemented on 2026-05-14
**Comparison source:** `Hmbown/DeepSeek-TUI` refreshed at `/tmp/deepseek-tui-compare-20260514`, HEAD `9483248a9f35b5f2b56c34b5b84fbc5334473c9d`.

## Gap

DeepSeek-TUI has a stronger first-run product surface around onboarding,
language, trust, and credential setup. DeepSeekCode already had `config init`,
`doctor`, model/provider pickers, and README install steps, but `doctor` did
not give a compact first-run checklist that tells a new user exactly what to do
next.

This slice improves the CLI onboarding surface without claiming full
interactive TUI onboarding parity.

## Implementation

- Add a human `[onboarding]` section to `deepseek doctor`.
- Add a stable `onboarding` object to `deepseek doctor --json`.
- Report whether project config and the configured model API key env var are
  present.
- Use explicit per-step `status` values (`done`, `missing`, `ready`, `blocked`)
  so runnable steps are not misreported as already completed.
- Emit non-secret next commands such as `deepseek config init`,
  `export DEEPSEEK_API_KEY=...`, `deepseek smoke`, and `deepseek tui`.
- Keep `doctor --json` side-effect-free and network-probe-free.
- Document the checklist in `docs/install.md`.

## Verification

- `cargo test json_report_includes_onboarding_next_steps_without_secret --lib`
- `cargo test json_report_is_valid_and_includes_stable_sections --lib`
- `cargo test json_report_skips_live_network_probe --lib`
- `cargo fmt --check`
- `git diff --check`

## Residual Gap

This is a CLI onboarding checklist. DeepSeek-TUI-style interactive first-run
language/API-key/trust setup inside the TUI remains a larger product polish
gap.
