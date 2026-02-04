//! Ability parsing and validation helpers.
//!
//! `#[move_struct]` can accept abilities in two places:
//! - Struct-level abilities via `abilities = "..."` (these drive which marker traits are
//!   implemented for the struct).
//! - Type-parameter abilities via `type_abilities = "T: ..."` or via normal Rust bounds on the
//!   type parameter (e.g. `T: HasStore + HasCopy`).
//!
//! This module contains the parsing and normalization logic (e.g. `copy` implies `drop`) plus
//! basic consistency checks (e.g. `key` and `copy` are incompatible).

use std::collections::BTreeSet;

#[derive(Clone, Copy)]
enum Ability {
    Key,
    Store,
    Copy,
    Drop,
}

/// Parsed ability flags (normalized to match Move rules).
#[derive(Default, Clone, Copy)]
pub(crate) struct AbilityFlags {
    pub(crate) key: bool,
    pub(crate) store: bool,
    pub(crate) copy: bool,
    pub(crate) drop: bool,
}

impl AbilityFlags {
    pub(crate) fn from_list(list: &[String], span: proc_macro2::Span) -> syn::Result<Self> {
        let mut flags = AbilityFlags::default();
        let mut seen = BTreeSet::new();

        for item in list {
            let ident = item.to_lowercase();
            if !seen.insert(ident.clone()) {
                continue;
            }

            let ability = match ident.as_str() {
                "key" => Ability::Key,
                "store" => Ability::Store,
                "copy" => Ability::Copy,
                "drop" => Ability::Drop,
                _ => {
                    return Err(syn::Error::new(
                        span,
                        format!(
                            "unknown ability `{ident}`; expected one of key, store, copy, drop"
                        ),
                    ))
                }
            };

            match ability {
                Ability::Key => flags.key = true,
                Ability::Store => flags.store = true,
                Ability::Copy => flags.copy = true,
                Ability::Drop => flags.drop = true,
            }
        }

        if flags.copy {
            flags.drop = true;
        }
        if flags.key && flags.copy {
            return Err(syn::Error::new(
                span,
                "a struct cannot have both `key` and `copy` abilities",
            ));
        }

        Ok(flags)
    }
}

pub(crate) fn parse_inline_abilities(param: &syn::TypeParam) -> syn::Result<Option<AbilityFlags>> {
    let mut flags = AbilityFlags::default();

    for bound in &param.bounds {
        if let syn::TypeParamBound::Trait(trait_bound) = bound {
            if let Some(seg) = trait_bound.path.segments.last() {
                let name = seg.ident.to_string().to_lowercase();
                match name.as_str() {
                    "hascopy" | "copyable" => flags.copy = true,
                    "hasdrop" | "droppable" => flags.drop = true,
                    "hasstore" | "storable" => flags.store = true,
                    "haskey" => flags.key = true,
                    _ => {}
                }
            }
        }
    }

    if flags.copy {
        flags.drop = true;
    }

    Ok((flags.key || flags.store || flags.copy || flags.drop).then_some(flags))
}
