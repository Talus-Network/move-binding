//! External package resolution for rendering cross-package type references.
//!
//! Codegen is deterministic by default: if a type reference points at an unknown external
//! package, rendering emits a `compile_error!` to force the caller to either:
//! - provide an external mapping, or
//! - generate bindings for that package as well.

use std::collections::BTreeMap;

use crate::ir::{Ability, NormalizedPackage, TypeName};

/// Resolver for packages that are not the one currently being rendered.
///
/// This lets the renderer:
/// - map external Move types to Rust paths like `dep_crate::module::Type`
/// - determine whether external types have the `key` ability (to generate object-arg signatures)
#[derive(Clone, Debug, Default)]
pub struct ExternalResolver {
    crate_by_address: BTreeMap<String, String>,
    has_key_by_type: BTreeMap<TypeName, bool>,
}

impl ExternalResolver {
    /// Create an empty resolver.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a package under a Rust crate name.
    ///
    /// Both `storage_id` and `original_id` (if present) are mapped to the same crate name to
    /// make resolution robust across package upgrades.
    pub fn add_package(&mut self, pkg: &NormalizedPackage, crate_name: impl Into<String>) {
        let crate_name = crate_name.into();
        self.crate_by_address
            .insert(pkg.storage_id.clone(), crate_name.clone());
        if let Some(orig) = &pkg.original_id {
            self.crate_by_address
                .insert(orig.clone(), crate_name.clone());
        }

        for module in pkg.modules.values() {
            for dt in &module.datatypes {
                let has_key = dt.abilities.contains(&Ability::Key);
                self.has_key_by_type.insert(dt.type_name.clone(), has_key);

                // Be robust to package upgrades: callers may reference either the storage id or
                // original id in type signatures.
                if let Some(orig) = &pkg.original_id {
                    if dt.type_name.address != *orig {
                        let mut alias = dt.type_name.clone();
                        alias.address = orig.clone();
                        self.has_key_by_type.insert(alias, has_key);
                    }
                }
                if dt.type_name.address != pkg.storage_id {
                    let mut alias = dt.type_name.clone();
                    alias.address = pkg.storage_id.clone();
                    self.has_key_by_type.insert(alias, has_key);
                }
            }
        }
    }

    /// Look up the Rust crate name for a Move package address.
    pub fn crate_name_for_address(&self, address: &str) -> Option<&str> {
        self.crate_by_address.get(address).map(|s| s.as_str())
    }

    /// Whether a fully-qualified Move type has the `key` ability.
    pub fn type_has_key(&self, type_name: &TypeName) -> Option<bool> {
        self.has_key_by_type.get(type_name).copied()
    }
}
