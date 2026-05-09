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
```

For source builds, `deepseek version` must match the version in `Cargo.toml`.

## Required Gates

Run the full local release gate before tagging or publishing:

```bash
cargo fmt --check
cargo test
deepseek benchmark
```

`deepseek benchmark` must pass all three layers:

- benchmark case expectations
- benchmark trend gate
- dogfood live gate

The live gate blocks release when new dogfood failures, stuck runs, or manual interventions appear after the previous benchmark snapshot.

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
```

Release notes should include:

- version
- commit SHA
- platform
- `deepseek version` output
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
mkdir -p ~/.local/bin/deepseek-rollback
cp "$(command -v deepseek)" ~/.local/bin/deepseek-rollback/deepseek.previous
```

Replace the binary, then validate:

```bash
deepseek version
deepseek doctor
```

Rollback:

```bash
cp ~/.local/bin/deepseek-rollback/deepseek.previous "$(command -v deepseek)"
deepseek version
deepseek doctor
```
