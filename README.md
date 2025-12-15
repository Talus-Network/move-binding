# move-binding

Layered crates for writing typed, ergonomic Move interactions on Sui from Rust.

This workspace is intentionally “deep”: each crate is a small abstraction that solves one problem,
and higher layers build on lower ones.

## Crates (low → high)

- `sui-move`: Move-shaped types (`MoveType`, `MoveStruct`, abilities, decoding helpers).
  See `sui-move/README.md`.
- `sui-move-derive`: derive macros for defining Move-shaped Rust structs.
  See `sui-move-derive/README.md`.
- `sui-move-call`: typed Move call descriptions (`CallSpec`) and `ToCallArg`.
  See `sui-move-call/README.md`.
- `sui-move-ptb`: build Sui `ProgrammableTransaction`s (PTBs) from `CallSpec`.
  See `sui-move-ptb/README.md`.
- `sui-move-runtime`: a “Rust-time vs Move-time” runtime for commit/simulate/inspect of PTBs and
  auto-updating runtime-owned object handles from transaction effects.
  See `sui-move-runtime/README.md`.

## Where to start

- Application code: start at `sui-move-runtime/README.md`.
- Interface crates (module/function wrappers): start at `sui-move-call/README.md`.
- Pure type modeling/decoding: start at `sui-move/README.md`.
