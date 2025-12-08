use proc_macro::TokenStream;
use quote::{format_ident, quote};
use std::collections::{BTreeMap, BTreeSet};
use syn::parse::{Parse, ParseStream, Parser};
use syn::parse_quote;
use syn::spanned::Spanned;
use syn::{
    parse_macro_input, Attribute, Data, DeriveInput, Field, FieldMutability, Fields, GenericParam,
    Lit, Meta, TypeParam,
};

fn parse_serde_attr(kind: &str) -> syn::Result<Attribute> {
    let ident = syn::Ident::new(kind, proc_macro2::Span::call_site());
    let meta: Meta = syn::parse_quote!(serde(#ident));
    Ok(Attribute {
        pound_token: Default::default(),
        style: syn::AttrStyle::Outer,
        bracket_token: Default::default(),
        meta,
    })
}

fn has_phantom_attr(attrs: &[Attribute]) -> bool {
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

#[derive(Clone, Copy)]
enum Ability {
    Key,
    Store,
    Copy,
    Drop,
}

#[derive(Default, Clone, Copy)]
struct AbilityFlags {
    key: bool,
    store: bool,
    copy: bool,
    drop: bool,
}

impl AbilityFlags {
    fn from_list(list: &[String], span: proc_macro2::Span) -> syn::Result<Self> {
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
        if flags.key && !flags.store {
            return Err(syn::Error::new(span, "ability `key` requires `store`"));
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

#[proc_macro_attribute]
pub fn move_module(_args: TokenStream, input: TokenStream) -> TokenStream {
    input
}

/// Attribute macro that turns a Rust struct into a Move-shaped type.
/// Usage:
/// ```ignore
/// #[move_struct(address = "0x1", module = "vault", abilities = "key, store", phantoms = "T")]
/// pub struct Vault<T> { ... }
/// ```
#[proc_macro_attribute]
pub fn move_struct(attr: TokenStream, item: TokenStream) -> TokenStream {
    let args = parse_macro_input!(attr as MoveStructArgs);
    let input = parse_macro_input!(item as DeriveInput);
    match expand_move_struct(args, input) {
        Ok(ts) => ts,
        Err(err) => err.to_compile_error().into(),
    }
}

#[derive(Default)]
struct MoveStructArgs {
    address: Option<String>,
    module: Option<String>,
    name: Option<String>,
    abilities: Vec<String>,
    phantoms: Vec<String>,
    type_abilities: BTreeMap<String, Vec<String>>,
    uid_type: Option<syn::Type>,
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
                                "uid_type must be a valid Rust type path, e.g., \"sui_move::types::UID\"",
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

fn parse_inline_abilities(param: &TypeParam) -> syn::Result<Option<AbilityFlags>> {
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

fn expand_move_struct(args: MoveStructArgs, input: DeriveInput) -> syn::Result<TokenStream> {
    let span = input.span();
    let struct_ident = input.ident.clone();
    let generics = input.generics.clone();
    let struct_vis = input.vis.clone();

    let data = match input.data.clone() {
        Data::Struct(s) => s,
        _ => {
            return Err(syn::Error::new(
                span,
                "#[move_struct] only supports structs",
            ))
        }
    };

    let original_fields = match &data.fields {
        Fields::Named(named) => named.named.clone().into_iter().collect::<Vec<_>>(),
        _ => unreachable!(),
    };

    let mut fields = match data.fields {
        Fields::Named(named) => named.named.into_iter().collect::<Vec<_>>(),
        _ => {
            return Err(syn::Error::new(
                span,
                "#[move_struct] currently supports only structs with named fields",
            ))
        }
    };

    // Inject PhantomData fields for phantom params
    let phantom_param_names: Vec<String> = args.phantoms.iter().map(|s| s.to_string()).collect();

    let phantom_params: Vec<&TypeParam> = generics
        .params
        .iter()
        .filter_map(|param| match param {
            GenericParam::Type(ty) => Some(ty),
            _ => None,
        })
        .filter(|ty| {
            let name = ty.ident.to_string();
            has_phantom_attr(&ty.attrs) || phantom_param_names.iter().any(|p| p == &name)
        })
        .collect();

    let mut type_ability_flags: BTreeMap<String, AbilityFlags> = args
        .type_abilities
        .iter()
        .map(|(name, abilities)| {
            AbilityFlags::from_list(abilities, span).map(|flags| (name.clone(), flags))
        })
        .collect::<syn::Result<_>>()?;

    // Merge inline generic ability annotations (#[abilities(copy, store)] T).
    for param in generics.params.iter().filter_map(|p| match p {
        GenericParam::Type(t) => Some(t),
        _ => None,
    }) {
        if let Some(flags) = parse_inline_abilities(param)? {
            type_ability_flags
                .entry(param.ident.to_string())
                .and_modify(|f| {
                    f.key |= flags.key;
                    f.store |= flags.store;
                    f.copy |= flags.copy;
                    f.drop |= flags.drop;
                })
                .or_insert(flags);
        }
    }

    for ty in &phantom_params {
        let ident = &ty.ident;
        let field_ident = format_ident!("phantom_{}", ident.to_string().to_lowercase());
        fields.push(Field {
            attrs: vec![parse_serde_attr("skip")?, parse_serde_attr("default")?],
            vis: struct_vis.clone(),
            mutability: FieldMutability::None,
            ident: Some(field_ident),
            colon_token: Some(Default::default()),
            ty: syn::parse_quote!(std::marker::PhantomData<#ident>),
        });
    }

    let abilities = AbilityFlags::from_list(&args.abilities, span)?;
    let has_key = abilities.key;
    let has_store = abilities.store;
    let has_copy = abilities.copy;
    let has_drop = abilities.drop;

    let type_param_idents: Vec<&TypeParam> = generics
        .params
        .iter()
        .filter_map(|p| match p {
            GenericParam::Type(t) => Some(t),
            _ => None,
        })
        .collect();

    for name in type_ability_flags.keys() {
        let target_ident = syn::Ident::new(name, span);
        let exists = type_param_idents
            .iter()
            .any(|param| param.ident == target_ident);
        if !exists {
            return Err(syn::Error::new(
                span,
                format!("type_abilities refers to unknown type parameter `{name}`"),
            ));
        }
    }

    let mut where_bounds: Vec<syn::WherePredicate> = Vec::new();
    for param in &type_param_idents {
        let ident = &param.ident;
        let mut bounds: Vec<syn::TypeParamBound> = vec![syn::parse_quote!(::sui_move::MoveType)];
        if let Some(abilities) = type_ability_flags.get(&ident.to_string()) {
            if abilities.copy {
                bounds.push(syn::parse_quote!(::sui_move::HasCopy));
            }
            if abilities.drop {
                bounds.push(syn::parse_quote!(::sui_move::HasDrop));
            }
            if abilities.store {
                bounds.push(syn::parse_quote!(::sui_move::HasStore));
            }
            if abilities.key {
                bounds.push(syn::parse_quote!(::sui_move::HasKey));
            }
        }
        where_bounds.push(syn::parse_quote!(#ident: #(#bounds)+*));
    }

    if has_key
        && !original_fields
            .iter()
            .any(|f| is_uid_field(f, args.uid_type.as_ref()))
    {
        return Err(syn::Error::new(
            span,
            "ability `key` requires a field `id: UID` (or uid_type override)",
        ));
    }

    // Ability constraints derived from struct-level abilities for each field (skip injected phantoms).
    for field in &original_fields {
        if is_phantom_field_type(&field.ty) {
            continue;
        }
        let ty = &field.ty;
        if has_copy {
            where_bounds.push(syn::parse_quote!(#ty: ::sui_move::HasCopy));
        }
        if has_drop {
            where_bounds.push(syn::parse_quote!(#ty: ::sui_move::HasDrop));
        }
        if has_store {
            where_bounds.push(syn::parse_quote!(#ty: ::sui_move::HasStore));
        }
    }

    let address = args
        .address
        .ok_or_else(|| syn::Error::new(span, "address is required in #[move_struct]"))?;
    let module_name = args
        .module
        .ok_or_else(|| syn::Error::new(span, "module is required in #[move_struct]"))?;
    let struct_name = args.name.unwrap_or_else(|| struct_ident.to_string());

    let ty_params_for_tag = type_param_idents
        .iter()
        .map(|p| {
            let ident = &p.ident;
            quote! { <#ident as ::sui_move::MoveType>::type_tag_static() }
        })
        .collect::<Vec<_>>();

    let struct_tag_builder = quote! {
        ::sui_sdk_types::StructTag::new(
            ::sui_move::parse_address(#address).expect("invalid address literal"),
            ::sui_move::parse_identifier(#module_name).expect("invalid module"),
            ::sui_move::parse_identifier(#struct_name).expect("invalid struct name"),
            vec![#(#ty_params_for_tag),*],
        )
    };

    let derives: Vec<syn::Path> = vec![
        parse_quote!(::core::clone::Clone),
        parse_quote!(::core::fmt::Debug),
        parse_quote!(::core::cmp::PartialEq),
        parse_quote!(::core::cmp::Eq),
        parse_quote!(::serde::Serialize),
        parse_quote!(::serde::Deserialize),
    ];

    let mut output_struct = input;
    output_struct.ident = struct_ident.clone();
    output_struct.generics = generics.clone();
    output_struct.data = Data::Struct(syn::DataStruct {
        struct_token: Default::default(),
        fields: Fields::Named(syn::FieldsNamed {
            brace_token: Default::default(),
            named: fields.clone().into_iter().collect(),
        }),
        semi_token: None,
    });
    output_struct
        .attrs
        .retain(|a| !a.path().is_ident("move_struct"));
    output_struct
        .attrs
        .push(parse_quote!(#[derive(#(#derives),*)]));

    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
    let where_clause = {
        let mut where_clause = where_clause.cloned();
        if !where_bounds.is_empty() {
            if let Some(ref mut w) = where_clause {
                w.predicates.extend(where_bounds.iter().cloned());
            } else {
                let preds = where_bounds.iter().cloned();
                where_clause = Some(syn::WhereClause {
                    where_token: Default::default(),
                    predicates: preds.collect(),
                });
            }
        }
        where_clause
    };

    let ability_impls = {
        let mut impls = Vec::new();
        if has_key {
            impls.push(quote! {
                impl #impl_generics ::sui_move::HasKey for #struct_ident #ty_generics #where_clause {}
            });
        }
        if has_store {
            impls.push(quote! {
                impl #impl_generics ::sui_move::HasStore for #struct_ident #ty_generics #where_clause {}
            });
        }
        if has_copy {
            impls.push(quote! {
                impl #impl_generics ::sui_move::HasCopy for #struct_ident #ty_generics #where_clause {}
            });
        }
        if has_drop {
            impls.push(quote! {
                impl #impl_generics ::sui_move::HasDrop for #struct_ident #ty_generics #where_clause {}
            });
        }
        quote! { #(#impls)* }
    };

    let expanded = quote! {
        #output_struct

        impl #impl_generics ::sui_move::MoveType for #struct_ident #ty_generics #where_clause {
            fn type_tag_static() -> ::sui_sdk_types::TypeTag {
                ::sui_sdk_types::TypeTag::Struct(Box::new(Self::struct_tag_static()))
            }
        }

        impl #impl_generics ::sui_move::MoveStruct for #struct_ident #ty_generics #where_clause {
            fn struct_tag_static() -> ::sui_sdk_types::StructTag {
                #struct_tag_builder
            }
        }

        #ability_impls
    };

    Ok(expanded.into())
}

fn is_uid_field(field: &Field, uid_override: Option<&syn::Type>) -> bool {
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

fn is_phantom_field_type(ty: &syn::Type) -> bool {
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
    quote::ToTokens::to_token_stream(a).to_string()
        == quote::ToTokens::to_token_stream(b).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn parse_args() {
        let args: MoveStructArgs = syn::parse_quote!(
            address = "0x1",
            module = "vault",
            abilities = "key, store",
            phantoms = "T",
            type_abilities = "T: store, copy"
        );
        assert_eq!(args.address.as_deref(), Some("0x1"));
        assert_eq!(args.module.as_deref(), Some("vault"));
        assert_eq!(args.name, None);
        assert_eq!(args.abilities, vec!["key".to_string(), "store".to_string()]);
        assert_eq!(args.phantoms, vec!["T".to_string()]);
        let mut expected = BTreeMap::new();
        expected.insert(
            "T".to_string(),
            vec!["store".to_string(), "copy".to_string()],
        );
        assert_eq!(args.type_abilities, expected);
    }
}
