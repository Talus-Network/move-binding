# move-binding

Layered crates for writing typed, ergonomic Move interactions on Sui from Rust.

Each crate is a small abstraction that solves one problem, and the crates can be adopted
individually.

The primary mental model is **Read → Tx → Commit** with a **cursor** that advances your local
frontier by applying transaction effects. See `MODEL.md`.

## Status

This is a pre-1.0 project. Public APIs may change before `1.0`; treat the crate boundaries and the
`MODEL.md` invariants as more stable than individual function names.

## Crates

Cargo package names use hyphens, while Rust imports use underscores.

| Cargo package | Rust import | Role |
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
talus-sui-move = { version = "=0.3.0", features = ["derive"] }
```

Use the same version for all `talus-sui-move*` crates in your application. Version `0.3`
uses the `0.4` series of `sui-sdk-types`, `sui-rpc`, and `sui-crypto`. If your application
passes their types or signers to these crates, update those direct dependencies to `0.4`
as well.

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
