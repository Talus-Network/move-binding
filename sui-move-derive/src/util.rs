//! Small helpers shared by parsing and code generation.
//!
//! These helpers are intentionally tiny and “dumb”: they exist to keep the expansion logic easy
//! to read.

use quote::ToTokens;
use syn::{parse_quote, Attribute, Field, Meta};

/// Construct a `#[serde(...)]` attribute.
pub(crate) fn parse_serde_attr(kind: &str) -> syn::Result<Attribute> {
    let ident = syn::Ident::new(kind, proc_macro2::Span::call_site());
    let meta: Meta = parse_quote!(serde(#ident));

    Ok(Attribute {
        pound_token: Default::default(),
        style: syn::AttrStyle::Outer,
        bracket_token: Default::default(),
        meta,
    })
}

/// Detect `#[phantom]` on generic parameters.
pub(crate) fn has_phantom_attr(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|a| {
        let path = a.path();
        path.is_ident("phantom")
            || path
                .segments
                .last()
                .map(|seg| seg.ident == "phantom")
                .unwrap_or(false)
    })
}

/// Whether a field is the `id: UID` field used for `key` objects.
pub(crate) fn is_uid_field(field: &Field, uid_override: Option<&syn::Type>) -> bool {
    let has_id_name = field.ident.as_ref().map(|i| i == "id").unwrap_or(false);
    if !has_id_name {
        return false;
    }

    if let Some(ty) = uid_override {
        return types_equal(&field.ty, ty);
    }

    match &field.ty {
        syn::Type::Path(p) => p
            .path
            .segments
            .last()
            .map(|seg| seg.ident == "UID")
            .unwrap_or(false),
        _ => false,
    }
}

/// Detect `PhantomData<...>` fields.
pub(crate) fn is_phantom_field_type(ty: &syn::Type) -> bool {
    match ty {
        syn::Type::Path(p) => p
            .path
            .segments
            .last()
            .map(|seg| seg.ident == "PhantomData")
            .unwrap_or(false),
        _ => false,
    }
}

fn types_equal(a: &syn::Type, b: &syn::Type) -> bool {
    a.to_token_stream().to_string() == b.to_token_stream().to_string()
}
