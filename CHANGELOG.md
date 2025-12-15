# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog], and this project adheres to
[Semantic Versioning].

## [Unreleased]

### Added

- Add `sui-move`: core Move-shaped type layer (traits, abilities, decoding).
- Add Move framework primitives under `sui_move::primitives` (e.g. `coin`, `balance`, `vec_map`).
- Add `sui-move-derive` and the `sui-move` `derive` feature for defining Move-shaped structs via macros.
- Add `sui-move-call`: typed Move call descriptions (`CallSpec`) plus typed wrappers for Sui `Input` kinds (pure, immutable/owned, shared, receiving).
- Add `sui-move-ptb`: minimal PTB builder that consumes `CallSpec` and produces `ProgrammableTransaction`.
- Add `sui-move-runtime`: runtime “Rust-time vs Move-time” namespace for commit/simulate/inspect of PTBs and auto-updating object handles from transaction effects.
- Add `Read::object_any` + `AnyObject<T>` convenience wrapper for owned/immutable vs shared objects (shared defaults to immutable; choose mutable via `as_shared_mutable`).

[Keep a Changelog]: https://keepachangelog.com/en/1.1.0/
[Semantic Versioning]: https://semver.org/spec/v2.0.0.html
