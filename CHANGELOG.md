# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog], and this project adheres to
[Semantic Versioning].

## [Unreleased]

### Added

- Add `sui-move`: core Move-shaped type layer (traits, abilities, decoding).
- Add `MoveType` + ability-marker impls for Move builtins (`u8/u16/u32/u64/u128`, `bool`, `Address`, `U256`, `Vec<T>`).
- Add `sui-move-derive` and the `sui-move` `derive` feature for defining Move-shaped structs via macros.
- Add `sui-move-call`: typed Move call descriptions (`CallSpec`) plus typed wrappers for Sui `Input` kinds (pure, immutable/owned, shared, receiving).
- Add `sui-move-ptb`: minimal PTB builder that consumes `CallSpec` and produces `ProgrammableTransaction`.
- Add `sui-move-runtime`: cursor-driven runtime for the Read → Tx → Commit mental model:
  - runtime-owned handles (`Object<T>`) that auto-update from transaction effects,
  - explicit input-mode views (`shared_immutable`, `shared_mutable`, `receiving`),
  - finality-aware `Receipt` plus recovery hooks (`Runtime::sync_transaction`, `Read::refresh*`),
  - ergonomic transaction macro (`sui_move_runtime::tx!`).
- Add `sui-move-codegen`: fetch + normalize package metadata (`NormalizedPackage`) and render typed Rust bindings (Move-shaped types + `CallSpec` builders).

[Keep a Changelog]: https://keepachangelog.com/en/1.1.0/
[Semantic Versioning]: https://semver.org/spec/v2.0.0.html
