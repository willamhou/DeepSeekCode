# DeepSeek-TUI Publish Readiness Status

## Context

Phase H still depends on external npm registry credentials, Homebrew tap
credentials, and real release matrix artifacts. Those actions should not be
guessed or performed from a local parity pass, but the remaining blockers need
to be visible and machine-checkable.

## Scope

- Add a read-only command that reports whether npm and Homebrew publishing can
  run instead of silently skipping in the tag workflow.
- Verify package metadata and version sync from tracked files.
- Verify optional release asset and npm artifact directories when provided.
- Verify an optional dogfood live evidence verification artifact when provided,
  so release readiness includes model-backed evidence instead of only packaging
  materials.
- Make strict mode fail when any publish prerequisite is blocked or skipped.
- Keep the command non-mutating: no tags, pushes, registry writes, or tap
  commits.

## Implementation

- `deepseek update publish-status` reports:
  - Cargo registry policy
  - npm metadata consistency
  - npm publish token availability
  - platform npm tarball availability when `--npm-dist` is provided
  - platform release archive and non-placeholder checksum availability when
    `--dist` is provided
  - live dogfood evidence verification with MCP loop-surface coverage when
    `--live-evidence-verification <path>` or `--live-evidence <path>` is
    provided
  - Homebrew formula template version
  - Homebrew tap repository/token availability
- `--strict` exits non-zero when any check is blocked or skipped.
- `--json` emits the same readiness checks as
  `deepseek.publish_status.v1` for CI and release scripts.
- Public install readiness for GitHub Release, npm, Homebrew, and GHCR now
  requires package materials and verified online dogfood evidence with MCP
  loop-surface coverage.
- `docs/release.md` and `docs/install.md` document the default and strict
  release readiness flows.

## Verification

- `/home/willamhou/.cargo/bin/cargo test update --lib`
- `cargo test publish_status --lib -- --test-threads=1`
- `/home/willamhou/.cargo/bin/cargo fmt --check`
- `deepseek update publish-status --json`
- `git diff --check`

## Remaining

Actual npm publication and Homebrew tap publication still require real external
registry/tap credentials plus a tagged release workflow with uploaded assets.
