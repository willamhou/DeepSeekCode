# Release Checklist

This checklist keeps a release tied to the same gates that protect the agent workflow.

## Preflight

Start from the remote mainline:

```bash
git fetch origin
git switch main
git merge --ff-only origin/main
```

Confirm the version and workspace health:

```bash
deepseek version
deepseek doctor
deepseek doctor --json
```

For source builds, `deepseek version` must match the version in `Cargo.toml`.
`deepseek doctor --json` must emit valid JSON for local supervisors and release automation.

## Required Gates

Run the full local release gate before tagging or publishing:

```bash
cargo fmt --check
cargo test
cargo package --allow-dirty
deepseek benchmark
```

`deepseek benchmark` must pass all three layers:

- benchmark case expectations
- benchmark trend gate
- dogfood live gate

The live gate blocks release when new dogfood failures, stuck runs, or manual interventions appear after the previous benchmark snapshot.
Failed benchmark gates do not advance the saved benchmark history baseline. After triaging known live failures, use
`deepseek benchmark --accept-live-baseline` only to intentionally accept the current dogfood snapshot; do not use it for normal release checks.

## Dogfood Replay

Replay at least one standard write/validate task and one retry task:

```bash
deepseek dogfood run --from-benchmark fixture-write-validate-rust-mini --notes "release replay"
deepseek dogfood run --from-benchmark fixture-retry-write-validate-python-mini --notes "release retry replay"
deepseek dogfood report --limit 5
```

If a replay exposes a new failure, fix the root cause before publishing. Do not release by overriding the dogfood outcome.

## Artifact

For a local release binary:

```bash
cargo build --release
./target/release/deepseek version
./target/release/deepseek doctor
./target/release/deepseek doctor --json
./target/release/deepseek update package --bin ./target/release/deepseek
./target/release/deepseek update verify-install --bin ./target/release/deepseek
./target/release/deepseek agents service --kind all --out target/service-smoke --bin ./target/release/deepseek --workdir "$PWD"
cargo package --allow-dirty
(cd npm && npm run check:version)
(cd npm && npm test)
DEEPSEEK_BINARY=./target/release/deepseek node npm/bin/deepseek.js version
node npm/scripts/stage-platform-package.js --platform linux-x64 --binary ./target/release/deepseek
node npm/scripts/verify-platform-package.js --platform linux-x64
```

For the runtime contract, start `./target/release/deepseek serve --http` and
capture `/health` plus `/runtime` from the release binary before publishing.

For the Docker artifact:

```bash
docker build -t deepseek-code:<version> .
docker run --rm deepseek-code:<version> version
```

Tag releases also publish the source-built image to GHCR through the Release
Matrix workflow:

```bash
docker pull ghcr.io/<owner>/<repo>:<version>
docker run --rm ghcr.io/<owner>/<repo>:<version> version
```

For the GitHub release matrix:

```bash
gh workflow run "Release Matrix"
gh run watch
```

The workflow builds and tests release binaries for Linux x64, macOS x64,
macOS arm64, and Windows x64. It also runs packaging checks for Cargo metadata,
`cargo package`, Cargo/npm/Homebrew version sync, the npm wrapper, root/platform
npm dry packaging, Homebrew formula syntax, Docker image build/run smoke, and
runtime service template rendering.
Each platform build also smoke-runs the binary after staging it into the
matching npm platform package, before packing the tarball that may be published
to npm.
Each platform artifact includes a sibling `.sha256` file, for example
`deepseek-macos-arm64.tar.gz.sha256`. The build job also creates GitHub signed
artifact attestations for each archive and checksum file.

When the workflow runs from a `v*` tag, it also creates or updates the matching
GitHub Release and uploads every platform archive plus checksum file as release
assets. It also packs platform npm packages from the compiled binaries. Manual
`workflow_dispatch` runs keep assets as workflow artifacts only. Tag runs also
publish a GHCR Docker image as `ghcr.io/<owner>/<repo>:<version>`,
`ghcr.io/<owner>/<repo>:v<version>`, and `ghcr.io/<owner>/<repo>:latest` with
OCI source, revision, and version labels. Tag runs also attempt `cargo publish`
after packaging checks and `npm publish` after platform package artifacts are
available. The crates.io publish step is skipped when the repository secret
`CARGO_REGISTRY_TOKEN` is not configured. The npm publish step is skipped when
`NPM_TOKEN` is not configured. The Homebrew tap publish step is skipped unless
`HOMEBREW_TAP_REPOSITORY` and `HOMEBREW_TAP_TOKEN` are configured; when enabled,
it renders `Formula/deepseek.rb` from the uploaded release checksums and pushes
it to the tap repository after the GitHub Release assets are published. The
Cargo and npm publish steps fail if the tag does not match the package version
they publish.

Verify downloaded release artifacts with:

```bash
gh attestation verify deepseek-macos-arm64.tar.gz --repo <owner>/<repo>
gh attestation verify deepseek-macos-arm64.tar.gz.sha256 --repo <owner>/<repo>
```

For the Homebrew formula:

```bash
ruby -c packaging/homebrew/deepseek.rb
deepseek update homebrew-formula \
  --version <version> \
  --repo <owner>/<repo> \
  --dist <downloaded-release-artifact-directory> \
  --formula packaging/homebrew/deepseek.rb
ruby -c packaging/homebrew/deepseek.rb
```

Before publishing a tap, download the release matrix `.sha256` files next to
their archives and run `deepseek update homebrew-formula`. The updater reads
`deepseek-linux-x64.tar.gz.sha256`, `deepseek-macos-x64.tar.gz.sha256`, and
`deepseek-macos-arm64.tar.gz.sha256`, then rewrites the formula with matching
release URLs and checksums.

To automate tap publishing from the tag workflow, set repository variable
`HOMEBREW_TAP_REPOSITORY` to the tap repository, for example
`owner/homebrew-tap`, and set secret `HOMEBREW_TAP_TOKEN` to a token with write
access to that repository.

Release notes should include:

- version
- commit SHA
- platform
- `deepseek version` output
- `deepseek doctor --json` output
- `deepseek serve --http` `/health` and `/runtime` output
- `release.json` from `deepseek update package`
- `SERVICES.md` and generated service-template smoke output
- `npm test` output from `npm/`
- root and platform npm package tarball names
- Docker image tag and `docker run ... version` output
- release matrix run URL, artifact names, `.sha256` file contents, and
  attestation verification output
- Homebrew formula SHA-256 values
- release gate result
- upgrade and rollback instructions

`deepseek` is the release artifact name. `dscode` is only a compatibility alias.

## Upgrade And Rollback

Source upgrade:

```bash
git pull
cargo install --path . --force
deepseek version
deepseek doctor
```

Binary upgrade:

```bash
deepseek update install-package --package target/deepseek-release/deepseek-<version>-<platform>
```

Replace the binary, then validate:

```bash
deepseek update verify-install --bin "$(command -v deepseek)"
```

Rollback:

```bash
deepseek update rollback
deepseek update verify-install --bin "$(command -v deepseek)"
```
