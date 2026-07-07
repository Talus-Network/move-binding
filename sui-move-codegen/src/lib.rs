#![doc = include_str!("../README.md")]
#![deny(missing_docs)]

//! See `README.md` for the crate-level overview.

mod source;
mod source_names;

/// Normalized, serde-friendly package IR.
pub mod ir;

/// Render normalized metadata into Rust source.
pub mod render;

pub use crate::source::fetch_package;
pub use crate::source_names::{
    apply_function_parameter_names_from_sources, function_parameter_names_from_sources,
    SourceNameError,
};

/// Sui gRPC client used by package metadata fetching.
pub type GrpcClient = sui_rpc::Client;

/// Errors from sourcing or normalizing package metadata.
#[derive(thiserror::Error, Debug)]
pub enum Error {
    /// gRPC request failed.
    #[error("grpc: {0}")]
    Grpc(String),

    /// Package missing from response.
    #[error("package missing from response")]
    MissingPackage,

    /// A fully-qualified Move type name could not be parsed.
    #[error("invalid type name: {0}")]
    InvalidTypeName(String),

    /// A required field was missing in the response.
    #[error("missing field: {0}")]
    MissingField(&'static str),

    /// Unknown ability enum value.
    #[error("unknown ability enum: {0}")]
    UnknownAbility(i32),

    /// Unknown visibility enum value.
    #[error("unknown visibility enum: {0}")]
    UnknownVisibility(i32),

    /// Unknown reference kind enum value.
    #[error("unknown reference kind: {0}")]
    UnknownReference(i32),

    /// Unknown datatype-kind enum value.
    #[error("unknown datatype kind: {0}")]
    UnknownDatatypeKind(i32),
}
