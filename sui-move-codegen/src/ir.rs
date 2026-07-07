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

fn replace_address<A, B>(address: &mut String, replacements: &[(A, B)])
where
    A: AsRef<str>,
    B: AsRef<str>,
{
    let normalized = normalize_address(address);
    if let Some((_, replacement)) = replacements
        .iter()
        .find(|(actual, _)| normalize_address(actual.as_ref()) == normalized)
    {
        *address = normalize_address(replacement.as_ref());
    } else {
        *address = normalized;
    }
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

impl TypeRef {
    fn replace_addresses<A, B>(&mut self, replacements: &[(A, B)])
    where
        A: AsRef<str>,
        B: AsRef<str>,
    {
        match self {
            TypeRef::Vector(inner) | TypeRef::Ref { inner, .. } => {
                inner.replace_addresses(replacements);
            }
            TypeRef::Datatype {
                type_name,
                type_arguments,
            } => {
                replace_address(&mut type_name.address, replacements);
                for ty in type_arguments {
                    ty.replace_addresses(replacements);
                }
            }
            TypeRef::Address
            | TypeRef::Bool
            | TypeRef::U8
            | TypeRef::U16
            | TypeRef::U32
            | TypeRef::U64
            | TypeRef::U128
            | TypeRef::U256
            | TypeRef::TypeParameter(_) => {}
        }
    }

    fn collect_addresses<'a>(&'a self, out: &mut Vec<&'a str>) {
        match self {
            TypeRef::Vector(inner) | TypeRef::Ref { inner, .. } => {
                inner.collect_addresses(out);
            }
            TypeRef::Datatype {
                type_name,
                type_arguments,
            } => {
                out.push(type_name.address.as_str());
                for ty in type_arguments {
                    ty.collect_addresses(out);
                }
            }
            TypeRef::Address
            | TypeRef::Bool
            | TypeRef::U8
            | TypeRef::U16
            | TypeRef::U32
            | TypeRef::U64
            | TypeRef::U128
            | TypeRef::U256
            | TypeRef::TypeParameter(_) => {}
        }
    }
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

/// Function parameter-name overlay failed because source and IR arities differ.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FunctionParameterNameMismatch {
    /// Move module name.
    pub module: String,
    /// Move function name.
    pub function: String,
    /// Number of parameters in the normalized IR.
    pub ir_count: usize,
    /// Number of parameters recovered from source.
    pub source_count: usize,
}

impl std::fmt::Display for FunctionParameterNameMismatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "`{}::{}` function parameter count mismatch for source names: IR has {}, source has {}",
            self.module, self.function, self.ir_count, self.source_count
        )
    }
}

impl std::error::Error for FunctionParameterNameMismatch {}

impl NormalizedPackage {
    /// Serialize the normalized package to pretty JSON.
    pub fn to_json_string(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Parse a normalized package from JSON.
    pub fn from_json_str(input: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(input)
    }

    /// Rewrite every package/type address in this IR using normalized `actual -> replacement`
    /// pairs.
    pub fn replace_addresses<A, B>(&mut self, replacements: &[(A, B)])
    where
        A: AsRef<str>,
        B: AsRef<str>,
    {
        replace_address(&mut self.storage_id, replacements);
        if let Some(original_id) = &mut self.original_id {
            replace_address(original_id, replacements);
        }

        for module in self.modules.values_mut() {
            for datatype in &mut module.datatypes {
                replace_address(&mut datatype.type_name.address, replacements);
                match &mut datatype.kind {
                    DatatypeKind::Struct { fields } => {
                        for field in fields {
                            field.ty.replace_addresses(replacements);
                        }
                    }
                    DatatypeKind::Enum { variants } => {
                        for variant in variants {
                            for field in &mut variant.fields {
                                field.ty.replace_addresses(replacements);
                            }
                        }
                    }
                }
            }

            for function in &mut module.functions {
                for parameter in &mut function.parameters {
                    parameter.ty.replace_addresses(replacements);
                }
                for return_type in &mut function.return_types {
                    return_type.replace_addresses(replacements);
                }
            }
        }
    }

    /// Return every package/type address referenced by this IR.
    pub fn referenced_addresses(&self) -> Vec<&str> {
        let mut addresses = vec![self.storage_id.as_str()];
        if let Some(original_id) = &self.original_id {
            addresses.push(original_id.as_str());
        }

        for module in self.modules.values() {
            for datatype in &module.datatypes {
                addresses.push(datatype.type_name.address.as_str());
                match &datatype.kind {
                    DatatypeKind::Struct { fields } => {
                        for field in fields {
                            field.ty.collect_addresses(&mut addresses);
                        }
                    }
                    DatatypeKind::Enum { variants } => {
                        for variant in variants {
                            for field in &variant.fields {
                                field.ty.collect_addresses(&mut addresses);
                            }
                        }
                    }
                }
            }

            for function in &module.functions {
                for parameter in &function.parameters {
                    parameter.ty.collect_addresses(&mut addresses);
                }
                for return_type in &function.return_types {
                    return_type.collect_addresses(&mut addresses);
                }
            }
        }

        addresses
    }

    /// Replace synthesized `argN` function parameter names with names recovered from Move source.
    pub fn apply_function_parameter_names(
        &mut self,
        names: &BTreeMap<(String, String), Vec<String>>,
    ) -> Result<(), FunctionParameterNameMismatch> {
        for (module_name, module) in &mut self.modules {
            for function in &mut module.functions {
                let Some(source_names) =
                    names.get(&(module_name.to_owned(), function.name.to_owned()))
                else {
                    continue;
                };

                if function.parameters.len() != source_names.len() {
                    return Err(FunctionParameterNameMismatch {
                        module: module_name.to_owned(),
                        function: function.name.to_owned(),
                        ir_count: function.parameters.len(),
                        source_count: source_names.len(),
                    });
                }

                for (index, (parameter, source_name)) in
                    function.parameters.iter_mut().zip(source_names).enumerate()
                {
                    if parameter.name == format!("arg{index}") {
                        parameter.name = source_name.to_owned();
                    }
                }
            }
        }

        Ok(())
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

        let encoded = pkg.to_json_string().expect("serialize");
        let decoded = NormalizedPackage::from_json_str(&encoded).expect("deserialize");
        assert_eq!(pkg, decoded);
        assert!(!encoded.is_empty());
    }

    #[test]
    fn replace_addresses_rewrites_package_and_nested_type_refs() {
        let mut pkg = NormalizedPackage {
            storage_id: "0x0009".into(),
            original_id: Some("0x9".into()),
            version: 1,
            modules: BTreeMap::from([(
                "m".into(),
                NormalizedModule {
                    name: "m".into(),
                    datatypes: vec![Datatype {
                        type_name: TypeName::parse("0x09::m::Obj").unwrap(),
                        module: "m".into(),
                        name: "Obj".into(),
                        abilities: vec![],
                        type_parameters: vec![],
                        kind: DatatypeKind::Struct {
                            fields: vec![Field {
                                name: "inner".into(),
                                position: 0,
                                ty: TypeRef::Datatype {
                                    type_name: TypeName::parse("0x0009::m::Inner").unwrap(),
                                    type_arguments: vec![TypeRef::Datatype {
                                        type_name: TypeName::parse("0x9::m::Leaf").unwrap(),
                                        type_arguments: vec![],
                                    }],
                                },
                            }],
                        },
                    }],
                    functions: vec![Function {
                        name: "use_obj".into(),
                        visibility: Visibility::Public,
                        is_entry: false,
                        type_parameters: vec![],
                        parameters: vec![FunctionParam {
                            name: "arg0".into(),
                            ty: TypeRef::Ref {
                                mutable: true,
                                inner: Box::new(TypeRef::Datatype {
                                    type_name: TypeName::parse("0x9::m::Obj").unwrap(),
                                    type_arguments: vec![],
                                }),
                            },
                        }],
                        return_types: vec![TypeRef::Datatype {
                            type_name: TypeName::parse("0x9::m::Obj").unwrap(),
                            type_arguments: vec![],
                        }],
                    }],
                },
            )]),
        };

        pkg.replace_addresses(&[("0x9", "0xa1")]);

        assert_eq!(pkg.storage_id, "0xa1");
        assert_eq!(pkg.original_id.as_deref(), Some("0xa1"));
        assert!(pkg
            .referenced_addresses()
            .into_iter()
            .all(|address| address == "0xa1"));
    }

    #[test]
    fn applies_source_names_only_to_synthesized_parameters() {
        let mut pkg = NormalizedPackage {
            storage_id: "0x1".into(),
            original_id: None,
            version: 1,
            modules: BTreeMap::from([(
                "m".into(),
                NormalizedModule {
                    name: "m".into(),
                    datatypes: vec![],
                    functions: vec![Function {
                        name: "f".into(),
                        visibility: Visibility::Public,
                        is_entry: true,
                        type_parameters: vec![],
                        parameters: vec![
                            FunctionParam {
                                name: "arg0".into(),
                                ty: TypeRef::U64,
                            },
                            FunctionParam {
                                name: "explicit".into(),
                                ty: TypeRef::Bool,
                            },
                        ],
                        return_types: vec![],
                    }],
                },
            )]),
        };
        let names = BTreeMap::from([(
            ("m".to_string(), "f".to_string()),
            vec!["amount".to_string(), "flag".to_string()],
        )]);

        pkg.apply_function_parameter_names(&names).unwrap();
        let parameters = &pkg.modules["m"].functions[0].parameters;

        assert_eq!(parameters[0].name, "amount");
        assert_eq!(parameters[1].name, "explicit");
    }
}
