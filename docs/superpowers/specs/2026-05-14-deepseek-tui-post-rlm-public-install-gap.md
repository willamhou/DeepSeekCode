# DeepSeek-TUI Post-RLM Public Install Gap

Status: implemented

## Baseline

Latest source comparison was refreshed against `Hmbown/DeepSeek-TUI` cloned to
`/tmp/deepseek-tui-compare-20260514`, HEAD
`81e4b93cc9df55de47489238078e255a563d044b`.

Compared with the earlier parity baseline, DeepSeek-TUI now presents a stronger
public distribution surface in addition to its CLI/TUI runtime surface:

- npm install path
- crates.io install path
- Homebrew tap path
- GitHub Release binary path
- GHCR Docker image path
- web/app-server packaging around the core CLI/TUI project

After the live RLM runtime event bridge, packaged RLM daemon service
documentation, and ACP `session/rlm/subscribe` extension, no open first-order
RLM/MCP/ACP subscription gap remains in DeepSeekCode. The remaining product gap
is now concentrated in public availability evidence rather than local release
readiness.

## Gap

DeepSeekCode already has local packaging templates and release workflow gates,
but `deepseek update publish-status` previously reported only publish readiness.
It did not make the distinction between:

- a source checkout that is publicly reachable,
- local artifacts/secrets that are ready to publish,
- install channels that still require an actual tag workflow or registry/tap
  publication, and
- the intentional Cargo registry source-build/package-only policy.

That made the parity plan too easy to overstate: a passing local publish
readiness check is not the same as verified public npm/Homebrew/GHCR/GitHub
Release availability.

## Implementation

- `deepseek update publish-status` now prints the repository slug and a
  `public_install` section.
- The JSON form now includes `repository` and `public_install` in
  `deepseek.publish_status.v1`.
- Public install checks use explicit statuses:
  - `source_available`
  - `ready_to_publish`
  - `requires_publish`
  - `source_only_policy`
- The checks cover source checkout, GitHub Release, npm, Homebrew, GHCR, and
  Cargo registry policy.
- Each public install check includes a concrete verification command so release
  runs can confirm the public channel before README/install docs advertise it as
  available.

## Verification

- `cargo test publish_status --lib`
- `cargo test update --lib`
- `cargo fmt --check`
- `cargo check`
- `git diff --check`
- `cargo run --bin deepseek -- update publish-status --json`
- `git ls-remote https://github.com/willamhou/DeepSeekCode.git HEAD`

## Remaining

Do not claim the DeepSeek-TUI distribution gap is below 5% until at least one
tagged release has live evidence for the public channels advertised in docs:

- GitHub Release assets and checksum files
- npm root and platform packages
- Homebrew tap formula with real release SHA-256 values
- GHCR image tags

Cargo registry publication remains intentionally out of scope while
`Cargo.toml` keeps `publish = false`.
