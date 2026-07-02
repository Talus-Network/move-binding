//! Code generation for `#[move_struct]`.
//!
//! The high-level expansion steps are:
//! 1. Validate input shape (named-field structs only).
//! 2. Inject `PhantomData` fields for phantom type parameters.
//! 3. Compute ability flags and generate appropriate `where`-bounds.
//! 4. Generate `MoveType`/`MoveStruct` impls and ability marker impls.
//!
//! The generated code references `::sui_move` (and its `__private` re-exports) so that downstream
//! crates don't need direct dependencies on `serde` or `sui-sdk-types`.

use std::collections::BTreeMap;

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::parse::Parser;
use syn::parse_quote;
use syn::spanned::Spanned;
use syn::{
    Data, DeriveInput, Field, FieldMutability, Fields, GenericParam, TypeParam, WhereClause,
    WherePredicate,
};

use crate::abilities::{parse_inline_abilities, AbilityFlags};
use crate::args::MoveStructArgs;
use crate::util::{has_phantom_attr, is_phantom_field_type, is_uid_field, parse_serde_attr};

pub(crate) fn expand_move_struct(
    args: MoveStructArgs,
    input: DeriveInput,
) -> syn::Result<TokenStream> {
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
            ty: parse_quote!(::std::marker::PhantomData<#ident>),
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

    let mut type_param_bounds: Vec<WherePredicate> = Vec::new();
    for param in &type_param_idents {
        let ident = &param.ident;
        let mut bounds: Vec<syn::TypeParamBound> = vec![parse_quote!(::sui_move::MoveType)];

        if let Some(abilities) = type_ability_flags.get(&ident.to_string()) {
            if abilities.copy {
                bounds.push(parse_quote!(::sui_move::HasCopy));
            }
            if abilities.drop {
                bounds.push(parse_quote!(::sui_move::HasDrop));
            }
            if abilities.store {
                bounds.push(parse_quote!(::sui_move::HasStore));
            }
            if abilities.key {
                bounds.push(parse_quote!(::sui_move::HasKey));
            }
        }

        type_param_bounds.push(parse_quote!(#ident: #(#bounds)+*));
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

    let mut store_bounds = type_param_bounds.clone();
    let mut copy_bounds = type_param_bounds.clone();
    let mut drop_bounds = type_param_bounds.clone();
    let key_bounds = type_param_bounds.clone();

    for field in &original_fields {
        if is_phantom_field_type(&field.ty) {
            continue;
        }
        let ty = &field.ty;
        if has_copy {
            copy_bounds.push(parse_quote!(#ty: ::sui_move::HasCopy));
        }
        if has_drop {
            drop_bounds.push(parse_quote!(#ty: ::sui_move::HasDrop));
        }
        if has_store {
            store_bounds.push(parse_quote!(#ty: ::sui_move::HasStore));
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

    let address_expr = if let Some(address_fn) = &args.address_fn {
        quote! { #address_fn() }
    } else {
        quote! { ::sui_move::parse_address(#address).expect("invalid address literal") }
    };

    let struct_tag_builder = quote! {
        ::sui_move::__private::sui_sdk_types::StructTag::new(
            #address_expr,
            ::sui_move::parse_identifier(#module_name).expect("invalid module"),
            ::sui_move::parse_identifier(#struct_name).expect("invalid struct name"),
            vec![#(#ty_params_for_tag),*],
        )
    };

    let derives: Vec<syn::Path> = vec![
        parse_quote!(::core::fmt::Debug),
        parse_quote!(::core::cmp::PartialEq),
        parse_quote!(::core::cmp::Eq),
        parse_quote!(::core::hash::Hash),
        parse_quote!(::sui_move::__private::serde::Serialize),
        parse_quote!(::sui_move::__private::serde::Deserialize),
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

    let mut serde_attrs = Vec::new();
    let mut other_attrs = Vec::new();
    for attr in output_struct.attrs.drain(..) {
        if attr.path().is_ident("serde") {
            serde_attrs.push(attr);
        } else {
            other_attrs.push(attr);
        }
    }

    let serde_has_crate_override = serde_attrs.iter().any(|attr| {
        let syn::Meta::List(list) = &attr.meta else {
            return false;
        };

        let mut found = false;
        let parser = syn::meta::parser(|meta| {
            if meta.path.is_ident("crate") {
                found = true;
            }

            if meta.input.peek(syn::Token![=]) {
                let _expr: syn::Expr = meta.value()?.parse()?;
            } else if meta.input.peek(syn::token::Paren) {
                let content;
                syn::parenthesized!(content in meta.input);
                let _tokens: proc_macro2::TokenStream = content.parse()?;
            }

            Ok(())
        });
        let _ = parser.parse2(list.tokens.clone());
        found
    });

    output_struct.attrs = other_attrs;
    output_struct
        .attrs
        .push(parse_quote!(#[derive(#(#derives),*)]));
    if !serde_has_crate_override {
        output_struct
            .attrs
            .push(parse_quote!(#[serde(crate = "sui_move::__private::serde")]));
    }
    output_struct.attrs.extend(serde_attrs);

    let mut clone_bounds = type_param_bounds.clone();
    clone_bounds.extend(
        original_fields
            .iter()
            .filter(|field| !is_phantom_field_type(&field.ty))
            .map(|field| {
                let ty = &field.ty;
                parse_quote!(#ty: ::core::clone::Clone)
            })
            .collect::<Vec<WherePredicate>>(),
    );
    let clone_where_clause = where_clause_with(&generics, &clone_bounds);

    let (impl_generics, ty_generics, _) = generics.split_for_impl();
    let move_type_where_clause = where_clause_with(&generics, &type_param_bounds);
    let key_where_clause = where_clause_with(&generics, &key_bounds);
    let store_where_clause = where_clause_with(&generics, &store_bounds);
    let copy_where_clause = where_clause_with(&generics, &copy_bounds);
    let drop_where_clause = where_clause_with(&generics, &drop_bounds);

    let ability_impls = {
        let mut impls = Vec::new();
        if has_key {
            impls.push(quote! {
                impl #impl_generics ::sui_move::HasKey for #struct_ident #ty_generics #key_where_clause {}
            });
        }
        if has_store {
            impls.push(quote! {
                impl #impl_generics ::sui_move::HasStore for #struct_ident #ty_generics #store_where_clause {}
            });
        }
        if has_copy {
            impls.push(quote! {
                impl #impl_generics ::sui_move::HasCopy for #struct_ident #ty_generics #copy_where_clause {}
            });
        }
        if has_drop {
            impls.push(quote! {
                impl #impl_generics ::sui_move::HasDrop for #struct_ident #ty_generics #drop_where_clause {}
            });
        }
        quote! { #(#impls)* }
    };

    let cloned_fields = fields.iter().map(|field| {
        let ident = field
            .ident
            .as_ref()
            .expect("move_struct supports only named fields");
        quote! { #ident: ::core::clone::Clone::clone(&self.#ident), }
    });

    Ok(quote! {
        #output_struct

        impl #impl_generics ::core::clone::Clone for #struct_ident #ty_generics #clone_where_clause {
            fn clone(&self) -> Self {
                Self {
                    #(#cloned_fields)*
                }
            }
        }

        impl #impl_generics ::sui_move::MoveType for #struct_ident #ty_generics #move_type_where_clause {
            fn type_tag_static() -> ::sui_move::__private::sui_sdk_types::TypeTag {
                ::sui_move::__private::sui_sdk_types::TypeTag::Struct(Box::new(
                    <Self as ::sui_move::MoveStruct>::struct_tag_static(),
                ))
            }
        }

        impl #impl_generics ::sui_move::MoveStruct for #struct_ident #ty_generics #move_type_where_clause {
            fn struct_tag_static() -> ::sui_move::__private::sui_sdk_types::StructTag {
                #struct_tag_builder
            }
        }

        #ability_impls
    })
}

fn where_clause_with(generics: &syn::Generics, bounds: &[WherePredicate]) -> Option<WhereClause> {
    let mut where_clause = generics.where_clause.clone();
    if bounds.is_empty() {
        return where_clause;
    }

    if let Some(ref mut existing) = where_clause {
        existing.predicates.extend(bounds.iter().cloned());
        return where_clause;
    }

    Some(WhereClause {
        where_token: Default::default(),
        predicates: bounds.iter().cloned().collect(),
    })
}
