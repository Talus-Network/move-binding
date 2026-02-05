//! Parsing for the `#[move_struct(...)]` attribute arguments.
//!
//! The proc-macro surface accepts a compact string-based syntax (e.g.
//! `abilities = "key, store"`, `type_abilities = "T: store, copy; U: drop"`). This module turns
//! those tokens into a structured representation and produces user-facing errors for malformed
//! input.

use std::collections::BTreeMap;

use syn::parse::{Parse, ParseStream, Parser};
use syn::spanned::Spanned;

#[derive(Clone)]
pub(crate) enum AddressArg {
    Literal(String),
    Expr(syn::Expr),
}

/// Parsed arguments for `#[move_struct(...)]`.
#[derive(Default)]
pub(crate) struct MoveStructArgs {
    pub(crate) address: Option<AddressArg>,
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
            let expr: syn::Expr = meta.value()?.parse()?;

            match ident.as_str() {
                "address" => {
                    match expr {
                        syn::Expr::Lit(expr_lit) => match expr_lit.lit {
                            syn::Lit::Str(s) => args.address = Some(AddressArg::Literal(s.value())),
                            other => {
                                return Err(syn::Error::new(other.span(), "address must be a string literal or an Address expression"));
                            }
                        },
                        other => {
                            args.address = Some(AddressArg::Expr(other));
                        }
                    }
                }
                "module" => {
                    let s = expect_lit_str(expr, "module")?;
                    args.module = Some(s.value());
                }
                "name" => {
                    let s = expect_lit_str(expr, "name")?;
                    args.name = Some(s.value());
                }
                "abilities" => {
                    let s = expect_lit_str(expr, "abilities")?;
                    args.abilities = s
                        .value()
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                }
                "phantoms" => {
                    let s = expect_lit_str(expr, "phantoms")?;
                    args.phantoms = s
                        .value()
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                }
                "type_abilities" => {
                    let s = expect_lit_str(expr, "type_abilities")?;
                    args.type_abilities = parse_type_abilities(&s.value(), s.span())?;
                }
                "uid_type" => {
                    let s = expect_lit_str(expr, "uid_type")?;
                    let ty: syn::Type = syn::parse_str(&s.value()).map_err(|_| {
                        syn::Error::new(
                            s.span(),
                            "uid_type must be a valid Rust type path, e.g., \"crate::UID\"",
                        )
                    })?;
                    args.uid_type = Some(ty);
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

fn expect_lit_str(expr: syn::Expr, key: &'static str) -> syn::Result<syn::LitStr> {
    match expr {
        syn::Expr::Lit(expr_lit) => match expr_lit.lit {
            syn::Lit::Str(s) => Ok(s),
            other => Err(syn::Error::new(
                other.span(),
                format!("{key} must be a string literal"),
            )),
        },
        other => Err(syn::Error::new(
            other.span(),
            format!("{key} must be a string literal"),
        )),
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
