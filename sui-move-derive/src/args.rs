//! Parsing for the `#[move_struct(...)]` attribute arguments.
//!
//! The proc-macro surface accepts a compact string-based syntax (e.g.
//! `abilities = "key, store"`, `type_abilities = "T: store, copy; U: drop"`). This module turns
//! those tokens into a structured representation and produces user-facing errors for malformed
//! input.

use std::collections::BTreeMap;

use syn::parse::{Parse, ParseStream, Parser};
use syn::spanned::Spanned;
use syn::Lit;

/// Parsed arguments for `#[move_struct(...)]`.
#[derive(Default)]
pub(crate) struct MoveStructArgs {
    pub(crate) address: Option<String>,
    pub(crate) module: Option<String>,
    pub(crate) name: Option<String>,
    pub(crate) abilities: Vec<String>,
    pub(crate) phantoms: Vec<String>,
    pub(crate) type_abilities: BTreeMap<String, Vec<String>>,
    pub(crate) uid_type: Option<syn::Type>,
}

impl Parse for MoveStructArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut args = MoveStructArgs::default();

        let parser = syn::meta::parser(|meta| {
            let ident = meta
                .path
                .get_ident()
                .ok_or_else(|| syn::Error::new(meta.path.span(), "expected identifier key"))?
                .to_string();
            let lit: Lit = meta.value()?.parse()?;

            match ident.as_str() {
                "address" => {
                    if let Lit::Str(s) = lit {
                        args.address = Some(s.value());
                    } else {
                        return Err(syn::Error::new(lit.span(), "address must be a string"));
                    }
                }
                "module" => {
                    if let Lit::Str(s) = lit {
                        args.module = Some(s.value());
                    } else {
                        return Err(syn::Error::new(lit.span(), "module must be a string"));
                    }
                }
                "name" => {
                    if let Lit::Str(s) = lit {
                        args.name = Some(s.value());
                    } else {
                        return Err(syn::Error::new(lit.span(), "name must be a string"));
                    }
                }
                "abilities" => {
                    if let Lit::Str(s) = lit {
                        args.abilities = s
                            .value()
                            .split(',')
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                            .collect();
                    } else {
                        return Err(syn::Error::new(
                            lit.span(),
                            "abilities must be a string literal, e.g., \"key, store\"",
                        ));
                    }
                }
                "phantoms" => {
                    if let Lit::Str(s) = lit {
                        args.phantoms = s
                            .value()
                            .split(',')
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                            .collect();
                    } else {
                        return Err(syn::Error::new(
                            lit.span(),
                            "phantoms must be a string literal, e.g., \"T, U\"",
                        ));
                    }
                }
                "type_abilities" => {
                    if let Lit::Str(ref s) = lit {
                        args.type_abilities = parse_type_abilities(&s.value(), lit.span())?;
                    } else {
                        return Err(syn::Error::new(
                            lit.span(),
                            "type_abilities must be a string literal, e.g., \"T: store, copy; U: drop\"",
                        ));
                    }
                }
                "uid_type" => {
                    if let Lit::Str(ref s) = lit {
                        let ty: syn::Type = syn::parse_str(&s.value()).map_err(|_| {
                            syn::Error::new(
                                s.span(),
                                "uid_type must be a valid Rust type path, e.g., \"crate::object::UID\"",
                            )
                        })?;
                        args.uid_type = Some(ty);
                    } else {
                        return Err(syn::Error::new(
                            lit.span(),
                            "uid_type must be a string literal path",
                        ));
                    }
                }
                other => {
                    return Err(syn::Error::new(
                        meta.path.span(),
                        format!("unknown attribute key `{other}`"),
                    ));
                }
            }

            Ok(())
        });

        let tokens: proc_macro2::TokenStream = input.parse()?;
        parser.parse2(tokens)?;

        Ok(args)
    }
}

fn parse_type_abilities(
    raw: &str,
    span: proc_macro2::Span,
) -> syn::Result<BTreeMap<String, Vec<String>>> {
    let mut map = BTreeMap::new();

    for entry in raw.split(';') {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }

        let mut parts = entry.splitn(2, ':');
        let name = parts
            .next()
            .ok_or_else(|| syn::Error::new(span, "expected type parameter name"))?
            .trim();
        let abilities = parts
            .next()
            .ok_or_else(|| syn::Error::new(span, "expected `:` followed by abilities"))?
            .trim();

        if name.is_empty() {
            return Err(syn::Error::new(span, "missing type parameter name"));
        }
        if abilities.is_empty() {
            return Err(syn::Error::new(
                span,
                "missing abilities after `:` in type_abilities",
            ));
        }

        let ability_list = abilities
            .split(',')
            .map(|a| a.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>();
        map.insert(name.to_string(), ability_list);
    }

    Ok(map)
}
