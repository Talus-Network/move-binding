# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog], and this project adheres to
[Semantic Versioning].

## [Unreleased]

## [0.2.0-rc.1] - 2026-08-27

### Added

- Add `talus-sui-move`: core layer for Move types, traits, abilities, and decoding.
- Keep Move framework declarations out of the `talus-sui-move` core so generated package bindings define
  `UID`, `Coin<T>`, `Balance<T>`, containers, and other framework shapes.
- Add `talus-sui-move-derive` and the `talus-sui-move` `derive` feature for defining Rust structs
  that represent Move structs.
- Add `talus-sui-move-call`: typed Move call descriptions (`CallSpec`) plus typed wrappers for Sui `Input` kinds (pure, immutable/owned, shared, receiving).
- Add `talus-sui-move-ptb`: minimal PTB builder that consumes `CallSpec` and produces `ProgrammableTransaction`.
- Add `talus-sui-move-runtime`: runtime with a cursor for the Read → Tx → Commit mental model:
  - handles owned by the runtime (`Object<T>`) that update from transaction effects,
  - explicit input mode views (`shared_immutable`, `shared_mutable`, `receiving`),
  - `Receipt` values that record finality plus recovery hooks (`Runtime::sync_transaction`,
    `Read::refresh*`),
  - ergonomic transaction macro (`talus_sui_move_runtime::tx!`).
- Add `talus-sui-move-codegen`: fetch and normalize package metadata (`NormalizedPackage`) and
  render typed Rust bindings for Move types and `CallSpec` builders.

### Changed

- Publish the workspace under the `talus-sui-move-*` crates.io package names owned by Talus and use
  the matching `talus_sui_move*` Rust library names.
- Generated package scopes now resolve type identity by module and datatype while calls continue to
  use the current package address.

[Keep a Changelog]: https://keepachangelog.com/en/1.1.0/
[Semantic Versioning]: https://semver.org/spec/v2.0.0.html
[Unreleased]: https://github.com/Talus-Network/move-binding/compare/v0.2.0-rc.1...HEAD
[0.2.0-rc.1]: https://github.com/Talus-Network/move-binding/compare/v0.1.0...v0.2.0-rc.1
