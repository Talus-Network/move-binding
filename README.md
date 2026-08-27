# move-binding

Layered crates for writing typed, ergonomic Move interactions on Sui from Rust.

This workspace is intentionally “deep”: each crate is a small abstraction that solves one problem,
and higher layers build on lower ones.

The primary mental model is **Read → Tx → Commit** with a **cursor** that advances your local
frontier by applying transaction effects. See `MODEL.md`.

## Status

The `0.2.0-rc.1` release candidate is the first release under crates.io package names owned by Talus.
Public APIs may change before `1.0`; treat the crate boundaries and the `MODEL.md` invariants as
more stable than individual function names.

## Packages (low → high)

The crates.io package names and Rust library names use the same Talus prefix. This gives consumers
one consistent name in Cargo and source code.

| crates.io package | Rust crate | Role |
| --- | --- | --- |
| `talus-sui-move` | `talus_sui_move` | Rust representations of Move types, abilities, and decoding helpers |
| `talus-sui-move-derive` | `talus_sui_move_derive` | Derive macros for Rust structs that represent Move structs |
| `talus-sui-move-call` | `talus_sui_move_call` | Typed Move call descriptions and arguments |
| `talus-sui-move-ptb` | `talus_sui_move_ptb` | Programmable transaction construction |
| `talus-sui-move-runtime` | `talus_sui_move_runtime` | Read/Tx/Commit runtime and typed handles |
| `talus-sui-move-codegen` | `talus_sui_move_codegen` | Rust binding generation from Move package metadata |

For example:

```toml
[dependencies]
talus-sui-move = { version = "=0.2.0-rc.1", features = ["derive"] }
```

```rust
use talus_sui_move::MoveType;
```

## Where to start

- Application code: start at [`sui-move-runtime/README.md`](sui-move-runtime/README.md).
- Interface crates (module/function wrappers): start at
  [`sui-move-call/README.md`](sui-move-call/README.md).
- Pure type modeling/decoding: start at [`sui-move/README.md`](sui-move/README.md).
- Generating bindings from package metadata on Sui: start at
  [`sui-move-codegen/README.md`](sui-move-codegen/README.md).

## Development

See `CONTRIBUTING.md` for local checks and pull request expectations.

## Security

See `SECURITY.md` for vulnerability reporting.

## License

Licensed under the Apache License, Version 2.0. See `LICENSE`.
