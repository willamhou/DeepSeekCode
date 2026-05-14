# Release Download Plan

## Context

DeepSeek-TUI v0.8.37 added China-friendly release fallback guidance around CNB
mirrors and binary asset mirrors. DeepSeekCode already has GitHub Release
assets, checksums, GHCR, npm metadata, and Homebrew formula generation, but the
CLI did not have a small operator-facing command for deriving the current
platform archive/checksum URLs or swapping in a private mirror directory.

## Spec

- Add `deepseek update download-plan`.
- Default to the current package version and `willamhou/DeepSeekCode`.
- Map supported release platforms to archive names:
  `linux-x64`, `macos-x64`, `macos-arm64`, and `windows-x64`.
- Support `--version`, `--repo`, `--platform`, `--base-url`, and `--json`.
- Use `DSCODE_RELEASE_BASE_URL` and `DSCODE_RELEASE_VERSION` as non-interactive
  fallback inputs when flags are omitted.
- Print archive URL, checksum URL, checksum command, and extraction command.
- Update install/release docs and multilingual README examples.

## Verification

- `cli_from_argv_routes_update_download_plan`
- `release_download_plan_uses_github_release_assets_by_default`
- `release_download_plan_accepts_mirror_base_url_and_windows_zip`
- `release_download_plan_json_renders_verify_fields`
- `cargo test update_download_plan --lib`
- `cargo test release_download_plan --lib`
- `cargo check`
- `cargo build --bin deepseek`
- `target/debug/deepseek update download-plan --version 0.1.1 --platform linux-x64`
- `target/debug/deepseek update download-plan --version 0.1.1 --platform windows-x64 --base-url https://mirror.example/releases/v0.1.1 --json`
- `cargo fmt --check`
- `cargo test --lib -- --test-threads=1`
- `git diff --check`
