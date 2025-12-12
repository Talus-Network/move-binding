//! Minimal mirrors of common Sui Move framework structs.
//!
//! These types exist to:
//! - construct correct Move `TypeTag`/`StructTag` values, and
//! - enable typed decoding of on-chain values into Rust.
//!
//! They are intentionally small “shape” types and do not provide any on-chain behavior.

pub mod ascii;
pub mod bag;
pub mod balance;
pub mod clock;
pub mod coin;
pub mod linked_table;
pub mod object_bag;
pub mod object_table;
pub mod sui;
pub mod tx_context;
pub mod type_name;
pub mod vec_map;
pub mod vec_set;
