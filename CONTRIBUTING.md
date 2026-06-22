# Contributing

This workspace is a set of layered Rust crates for typed Sui Move interactions.
Changes should preserve the layer boundaries described in `MODEL.md`: low-level Move type
modeling should not depend on transaction building or runtime behavior, and runtime code should
not hide the Read -> Tx -> Commit boundary.

## Local Checks

Run these before opening a pull request:

```sh
cargo fmt --all -- --check
cargo clippy --all --all-targets -- -D warnings
cargo test --all --all-targets
cargo test --doc --all
```

CI runs formatting and clippy strictly, plus stable Rust checks and tests.

## Pull Requests

- Keep changes scoped to one behavioral concern.
- Add or update tests for public API behavior, generated output, transaction construction, and
  runtime cursor/effects handling.
- Update the relevant crate README when changing user-facing APIs.
- Update `CHANGELOG.md` for externally visible changes.

## Dependencies

Prefer crates.io dependencies with explicit versions. Git dependencies are acceptable only when
they are intentional and should also include a version requirement when the crate exists on
crates.io, so the workspace can be packaged predictably.

## Publishing

Publish crates in dependency order:

1. `sui-move-derive`
2. `sui-move`
3. `sui-move-call`
4. `sui-move-ptb`
5. `sui-move-runtime`
6. `sui-move-codegen`

Before the first crates.io release, `cargo package --workspace` is expected to fail for crates
whose normal dependencies are not published yet. Package each crate after its normal dependencies
are available in the registry.
