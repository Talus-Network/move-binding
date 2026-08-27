# Releasing crates

All workspace crates use one version and one Git tag.

## Verify

```console
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-targets --all-features --locked
cargo test --doc --workspace --all-features --locked
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps --locked
./scripts/check-packages.sh
```

## Tag

Merge the release commit into `main`, then create and push a signed tag:

```console
git tag -s v0.2.0-rc.1 -m "move-binding v0.2.0-rc.1"
git push origin v0.2.0-rc.1
```

## First publication

From the clean tagged commit:

```console
cargo login
PUBLISH_CRATES_CONFIRM=v0.2.0-rc.1 \
  ./scripts/publish-crates.sh v0.2.0-rc.1
```

After publication:

1. Add `devops-talus` and `github:Talus-Network:engineering` as owners of every crate.
2. Configure every crate to trust `Talus-Network/move-binding`, workflow
   `publish-crates.yml`, environment `crates-io`.
3. Revoke the personal token and run `cargo logout`.

Use `ALLOW_EXISTING=1` only to resume a reviewed partial publication.

## Later releases

Create and push the signed tag, then dispatch `Publish crates.io packages` in GitHub Actions.
Create the GitHub release only after every crate version is visible on crates.io.
