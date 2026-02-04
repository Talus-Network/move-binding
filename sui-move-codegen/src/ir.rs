//! A small, serde-friendly IR for Move package metadata.
//!
//! This module intentionally models only what code generation needs:
//! - Move datatypes (structs/enums) with field layouts and abilities
//! - Move function signatures (type params + parameter/return types)
//!
//! The IR is designed to be persisted as JSON so builds can be deterministic (no network access).

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Move abilities.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Ability {
    /// `copy`
    Copy,
    /// `drop`
    Drop,
    /// `store`
    Store,
    /// `key`
    Key,
}

/// A fully-qualified type name: `<addr>::<module>::<name>`.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct TypeName {
    /// `0x...` address.
    ///
    /// Addresses are normalized to a canonical `0x...` form (leading zeros are removed).
    pub address: String,
    /// Move module identifier.
    pub module: String,
    /// Type name.
    pub name: String,
}

impl TypeName {
    /// Parse a fully-qualified type name like `0x2::object::UID`.
    ///
    /// This normalizes the address portion to the same canonical `0x...` representation used by
    /// the rest of the IR.
    pub fn parse(input: &str) -> Option<Self> {
        let mut parts = input.split("::");
        let address = parts.next()?.to_string();
        let module = parts.next()?.to_string();
        let name = parts.next()?.to_string();
        if parts.next().is_some() {
            return None;
        }
        Some(Self {
            address: normalize_address(&address),
            module,
            name,
        })
    }
}

fn normalize_address(input: &str) -> String {
    let trimmed = input.trim();
    let addr = trimmed
        .trim_start_matches("0x")
        .trim_start_matches("0X")
        .trim_start_matches('0');
    let addr = if addr.is_empty() { "0" } else { addr };
    format!("0x{addr}")
}

/// A Move type reference from package metadata.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TypeRef {
    /// `address`
    Address,
    /// `bool`
    Bool,
    /// `u8`
    U8,
    /// `u16`
    U16,
    /// `u32`
    U32,
    /// `u64`
    U64,
    /// `u128`
    U128,
    /// `u256`
    U256,
    /// `vector<T>`
    Vector(Box<TypeRef>),
    /// `&T` or `&mut T`
    Ref {
        /// `true` if `&mut`.
        mutable: bool,
        /// Referenced type.
        inner: Box<TypeRef>,
    },
    /// `0x...::module::Name<T0, ...>`
    Datatype {
        /// Fully-qualified name.
        type_name: TypeName,
        /// Type arguments.
        type_arguments: Vec<TypeRef>,
    },
    /// A generic type parameter index.
    ///
    /// The index corresponds to the type parameter position in the surrounding signature.
    TypeParameter(u32),
}

/// A function parameter (name is synthesized; Sui metadata does not carry parameter names).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FunctionParam {
    /// Parameter name (`arg0`, `arg1`, ...).
    pub name: String,
    /// Parameter type.
    pub ty: TypeRef,
}

/// Function visibility.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Visibility {
    /// `private`
    Private,
    /// `public`
    Public,
    /// `public(friend)`
    Friend,
}

/// A Move function signature.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Function {
    /// Function name.
    pub name: String,
    /// Visibility.
    pub visibility: Visibility,
    /// Whether the function is `entry`.
    pub is_entry: bool,
    /// Generic type parameters.
    pub type_parameters: Vec<TypeParameter>,
    /// Function parameters.
    pub parameters: Vec<FunctionParam>,
    /// Return types.
    pub return_types: Vec<TypeRef>,
}

/// A generic type parameter definition.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TypeParameter {
    /// Ability constraints (`T: store + drop`, etc).
    pub constraints: Vec<Ability>,
    /// Whether the parameter is phantom.
    pub is_phantom: bool,
}

/// Struct or enum layout.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DatatypeKind {
    /// Struct layout.
    Struct {
        /// Fields.
        fields: Vec<Field>,
    },
    /// Enum layout.
    Enum {
        /// Variants.
        variants: Vec<Variant>,
    },
}

/// A struct field.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Field {
    /// Field name.
    pub name: String,
    /// Field position in the Move definition.
    pub position: u32,
    /// Field type.
    pub ty: TypeRef,
}

/// An enum variant.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Variant {
    /// Variant name.
    pub name: String,
    /// Variant position in the Move definition.
    pub position: u32,
    /// Variant fields.
    pub fields: Vec<Field>,
}

/// A Move datatype definition.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Datatype {
    /// Fully-qualified type name.
    pub type_name: TypeName,
    /// Move module name (redundant but convenient).
    pub module: String,
    /// Datatype name (redundant but convenient).
    pub name: String,
    /// Abilities.
    pub abilities: Vec<Ability>,
    /// Type parameters.
    pub type_parameters: Vec<TypeParameter>,
    /// Layout.
    pub kind: DatatypeKind,
}

/// A Move module.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NormalizedModule {
    /// Module name.
    pub name: String,
    /// Datatypes defined in this module.
    pub datatypes: Vec<Datatype>,
    /// Public/entry functions (or all functions, depending on source).
    #[serde(default)]
    pub functions: Vec<Function>,
}

/// A Move package.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NormalizedPackage {
    /// Storage id of this package version (on-chain object id).
    ///
    /// This is the concrete package object id for a specific published version.
    pub storage_id: String,
    /// Original id of the package (stable across versions), if provided.
    ///
    /// If a package has been upgraded, `original_id` stays the same while `storage_id` changes.
    /// Codegen uses both values to decide whether a type reference should be treated as “local”.
    pub original_id: Option<String>,
    /// Version number.
    pub version: u64,
    /// Modules by name.
    pub modules: BTreeMap<String, NormalizedModule>,
}

impl NormalizedPackage {
    /// Serialize the normalized package to pretty JSON.
    pub fn to_json_string(&self) -> serde_json::Result<String> {
        serde_json::to_string_pretty(self)
    }

    /// Parse a normalized package from JSON.
    pub fn from_json_str(input: &str) -> serde_json::Result<Self> {
        serde_json::from_str(input)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn json_roundtrip() {
        let pkg = NormalizedPackage {
            storage_id: "0x1".into(),
            original_id: Some("0x1".into()),
            version: 42,
            modules: BTreeMap::from([(
                "m".into(),
                NormalizedModule {
                    name: "m".into(),
                    datatypes: vec![Datatype {
                        type_name: TypeName {
                            address: "0x1".into(),
                            module: "m".into(),
                            name: "S".into(),
                        },
                        module: "m".into(),
                        name: "S".into(),
                        abilities: vec![Ability::Store, Ability::Key],
                        type_parameters: vec![TypeParameter {
                            constraints: vec![Ability::Copy, Ability::Drop],
                            is_phantom: true,
                        }],
                        kind: DatatypeKind::Struct {
                            fields: vec![Field {
                                name: "id".into(),
                                position: 0,
                                ty: TypeRef::Datatype {
                                    type_name: TypeName {
                                        address: "0x2".into(),
                                        module: "object".into(),
                                        name: "UID".into(),
                                    },
                                    type_arguments: vec![],
                                },
                            }],
                        },
                    }],
                    functions: vec![Function {
                        name: "foo".into(),
                        visibility: Visibility::Public,
                        is_entry: true,
                        type_parameters: vec![TypeParameter {
                            constraints: vec![Ability::Copy],
                            is_phantom: false,
                        }],
                        parameters: vec![FunctionParam {
                            name: "arg0".into(),
                            ty: TypeRef::Ref {
                                mutable: false,
                                inner: Box::new(TypeRef::U64),
                            },
                        }],
                        return_types: vec![TypeRef::Datatype {
                            type_name: TypeName {
                                address: "0x1".into(),
                                module: "m".into(),
                                name: "S".into(),
                            },
                            type_arguments: vec![TypeRef::TypeParameter(0)],
                        }],
                    }],
                },
            )]),
        };

        let json = pkg.to_json_string().expect("serialize");
        let decoded = NormalizedPackage::from_json_str(&json).expect("deserialize");
        assert_eq!(pkg, decoded);
        assert!(json.contains("\"storage_id\""));
        assert!(json.contains("\"modules\""));
    }
}
