use proc_macro::TokenStream;
use quote::{format_ident, quote};
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

    let abilities = args
        .abilities
        .iter()
        .map(|a| a.to_lowercase())
        .collect::<Vec<_>>();

    let has_key = abilities.iter().any(|a| a == "key");
    let has_store = abilities.iter().any(|a| a == "store");
    let has_copy = abilities.iter().any(|a| a == "copy");
    let has_drop = abilities.iter().any(|a| a == "drop");

    let type_param_idents: Vec<&TypeParam> = generics
        .params
        .iter()
        .filter_map(|p| match p {
            GenericParam::Type(t) => Some(t),
            _ => None,
        })
        .collect();

    let where_bounds: Vec<syn::WherePredicate> = type_param_idents
        .iter()
        .map(|p| {
            let ident = &p.ident;
            syn::parse_quote!(#ident: ::sui_move::MoveType)
        })
        .collect();

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_args() {
        let args: MoveStructArgs = syn::parse_quote!(
            address = "0x1",
            module = "vault",
            abilities = "key, store",
            phantoms = "T"
        );
        assert_eq!(args.address.as_deref(), Some("0x1"));
        assert_eq!(args.module.as_deref(), Some("vault"));
        assert_eq!(args.name, None);
        assert_eq!(args.abilities, vec!["key".to_string(), "store".to_string()]);
        assert_eq!(args.phantoms, vec!["T".to_string()]);
    }
}
