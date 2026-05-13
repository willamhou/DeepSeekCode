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
  - Homebrew formula template version
  - Homebrew tap repository/token availability
- `--strict` exits non-zero when any check is blocked or skipped.
- `docs/release.md` and `docs/install.md` document the default and strict
  release readiness flows.

## Verification

- `/home/willamhou/.cargo/bin/cargo test update --lib`
- `/home/willamhou/.cargo/bin/cargo fmt --check`
- `git diff --check`

## Remaining

Actual npm publication and Homebrew tap publication still require real external
registry/tap credentials plus a tagged release workflow with uploaded assets.
