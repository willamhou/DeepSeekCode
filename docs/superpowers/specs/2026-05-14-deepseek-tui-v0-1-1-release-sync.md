# DeepSeek-TUI v0.1.1 Release Sync

**Status:** implemented on 2026-05-14
**Comparison source:** `Hmbown/DeepSeek-TUI` refreshed at `/tmp/deepseek-tui-compare-20260514`, HEAD `9483248a9f35b5f2b56c34b5b84fbc5334473c9d`.

## Gap

The public `v0.1.0` release does not include the latest onboarding parity
work: CLI stdin auth persistence, masked TUI auth, and `/setup wizard`.

This slice prepares a synchronized `v0.1.1` release candidate so the next tag
can publish those capabilities through the GitHub Release and GHCR paths.

## Implementation

- Bump `Cargo.toml` and `Cargo.lock` to `0.1.1`.
- Bump the npm root package, optional dependency pins, and every platform
  package to `0.1.1`.
- Bump the local Homebrew formula template URLs to `v0.1.1`.
- Make the release workflow Homebrew render smoke read the version from
  `Cargo.toml` instead of hardcoding the version in workflow YAML.

## Verification

- `cd npm && npm run check:version`
- `cd npm && npm test`
- `cargo check`
- `cargo build`
- `cargo fmt --check`
- `git diff --check`
- `target/debug/deepseek version` prints `deepseek 0.1.1`

## Residual Gap

The `v0.1.1` public release still requires creating and pushing the tag, waiting
for the release workflow, and then smoke-testing the generated GitHub Release
assets and GHCR image before README install links should be moved from
`v0.1.0` to `v0.1.1`.
