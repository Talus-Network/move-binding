# Contributing to `move-binding`

Thank you for considering contributing to this project. These guidelines keep the process clear
and preserve the structure of the workspace.

This workspace is a set of layered Rust crates for typed Sui Move interactions. Changes should
preserve the layer boundaries described in `MODEL.md`: low-level Move type modeling should not
depend on transaction building or runtime behavior, and runtime code should not hide the
Read -> Tx -> Commit boundary.

## Code of Conduct

This project adheres to a [Code of Conduct]. By participating, you are expected to uphold this
code.

## How to Contribute

1. Fork the repository and create your branch from `main`.
2. Follow the coding style used in the project.
3. Write clear, concise commit messages.
4. Add tests for new functionality or behavior changes.
5. Do not introduce `unsafe` Rust unless the safety invariant is documented and reviewed
   explicitly.
6. Update documentation to complement user-facing code changes.
7. Update `CHANGELOG.md` for externally visible changes.

## Local Checks

Run these before opening a pull request:

```sh
cargo fmt --all -- --check
cargo clippy --all --all-targets -- -D warnings
cargo test --all --all-targets
cargo test --doc --all
```

CI runs formatting and clippy strictly, plus stable Rust checks and tests.

## Reporting Issues

- Use the [Issue Tracker] to report bugs or suggest enhancements.
- Check if the issue already exists before submitting.
- Provide steps to reproduce, expected vs. actual behavior, and relevant logs or screenshots.

## Pull Request Process

- Open a pull request with a clear description of what it does and why.
- Link to the issue it fixes, if applicable.
- Keep changes scoped to one behavioral concern.
- Ensure your branch is up to date with `main`.
- Add or update tests for public API behavior, generated output, transaction construction, and
  runtime cursor/effects handling.
- Update the relevant crate README when changing user-facing APIs.
- A maintainer will review and may request changes before merge.

## Writing Commit Messages

- Use the [Conventional Commits] specification for commit messages.
- Use the present tense, for example `feat: add typed call builder` instead of
  `feat: added typed call builder`.
- Use imperative tone, for example `fix: reject duplicate object inputs` instead of
  `fix: rejects duplicate object inputs`.
- Reference a ticket if applicable.
- Add context and an explanation for non-trivial contributions.

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

## License

By contributing, you agree that your contributions will be licensed under the same license as the
project: [LICENSE].

[Issue Tracker]: https://github.com/Talus-Network/move-binding/issues
[Code of Conduct]: CODE_OF_CONDUCT.md
[LICENSE]: LICENSE
[Conventional Commits]: https://www.conventionalcommits.org/
