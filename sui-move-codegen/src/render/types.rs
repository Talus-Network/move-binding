//! Datatype rendering (`struct`/`enum`) for generated bindings.
//!
//! Structs are emitted using `#[sui_move_derive::move_struct]` so the generated code automatically
//! gets `MoveType` / `MoveStruct` implementations plus ability marker traits.
//!
//! Enums are emitted as Rust `enum`s with manual `MoveType` / `MoveStruct` impls. (Move enum
//! support is still evolving, so this layer keeps the implementation small and explicit.)

use proc_macro2::TokenStream;
use quote::{format_ident, quote};

use crate::ir::{Ability, Datatype, DatatypeKind, Field, NormalizedPackage, TypeName, TypeRef};

use super::{builtins, idents, RenderOptions};

pub(crate) fn render_datatype(
    dt: &Datatype,
    pkg: &NormalizedPackage,
    opts: &RenderOptions,
) -> TokenStream {
    match &dt.kind {
        DatatypeKind::Struct { fields } => render_struct(dt, fields, pkg, opts),
        DatatypeKind::Enum { variants } => render_enum(dt, variants, pkg, opts),
    }
}

pub(crate) fn render_type_ref_in_module(
    ty: &TypeRef,
    current_module: &str,
    pkg: &NormalizedPackage,
    opts: &RenderOptions,
) -> TokenStream {
    let current = TypeName {
        address: pkg.storage_id.clone(),
        module: current_module.to_string(),
        name: "<current>".to_string(),
    };
    render_type_ref(ty, &current, pkg, opts)
}

fn render_struct(
    dt: &Datatype,
    fields: &[Field],
    pkg: &NormalizedPackage,
    opts: &RenderOptions,
) -> TokenStream {
    let type_ident = idents::ident(&dt.name);
    let type_params = type_params_idents(dt.type_parameters.len());
    let type_generics = type_generics(&type_params);

    let address = &dt.type_name.address;
    let module = &dt.type_name.module;
    let move_name = &dt.type_name.name;

    let abilities = abilities_string(&dt.abilities);
    let phantoms = phantom_params_string(dt);
    let type_abilities = type_abilities_string(dt);

    let address_lit = syn::LitStr::new(address, proc_macro2::Span::call_site());
    let module_lit = syn::LitStr::new(module, proc_macro2::Span::call_site());
    let name_lit = syn::LitStr::new(move_name, proc_macro2::Span::call_site());
    let abilities_lit = abilities
        .as_deref()
        .map(|s| syn::LitStr::new(s, proc_macro2::Span::call_site()));
    let phantoms_lit = phantoms
        .as_deref()
        .map(|s| syn::LitStr::new(s, proc_macro2::Span::call_site()));
    let type_abilities_lit = type_abilities
        .as_deref()
        .map(|s| syn::LitStr::new(s, proc_macro2::Span::call_site()));

    let need_name_override = idents::is_keyword(&dt.name);
    let name_arg = need_name_override.then(|| quote! { name = #name_lit, });
    let abilities_arg = abilities_lit.map(|lit| quote! { abilities = #lit, });
    let phantoms_arg = phantoms_lit.map(|lit| quote! { phantoms = #lit, });
    let type_abilities_arg = type_abilities_lit.map(|lit| quote! { type_abilities = #lit, });

    let fields_tokens = fields.iter().map(|f| {
        let ident = idents::ident(&f.name);
        let ty = render_type_ref(&f.ty, &dt.type_name, pkg, opts);
        quote! { pub #ident: #ty, }
    });

    let doc = doc_lines(&[
        format!("Move type: `{address}::{module}::{move_name}`."),
        abilities
            .as_deref()
            .map(|a| format!("Abilities: `{a}`."))
            .unwrap_or_else(|| "Abilities: *(none)*.".to_string()),
    ]);

    quote! {
        #doc
        #[sui_move_derive::move_struct(
            address = #address_lit,
            module = #module_lit,
            #name_arg
            #abilities_arg
            #phantoms_arg
            #type_abilities_arg
        )]
        pub struct #type_ident #type_generics {
            #(#fields_tokens)*
        }
    }
}

fn render_enum(
    dt: &Datatype,
    variants: &[crate::ir::Variant],
    pkg: &NormalizedPackage,
    opts: &RenderOptions,
) -> TokenStream {
    let type_ident = idents::ident(&dt.name);
    let type_params = type_params_idents(dt.type_parameters.len());
    let (impl_generics, type_generics) = impl_and_type_generics(&type_params);

    let address = &dt.type_name.address;
    let module = &dt.type_name.module;
    let move_name = &dt.type_name.name;

    let abilities = abilities_string(&dt.abilities);
    let bounds = type_param_bounds(dt, opts.use_aliases);

    let sm = if opts.use_aliases {
        quote! { sm }
    } else {
        quote! { sui_move }
    };
    let struct_tag_builder = struct_tag_builder_tokens(dt, opts.use_aliases);
    let where_clause = where_clause(&bounds);

    let derives = enum_derives(&dt.abilities, opts.use_aliases);
    let serde_crate_attr = serde_crate_attr();

    let variants_tokens = variants.iter().map(|v| {
        let variant_ident = idents::ident(&v.name);
        if v.fields.is_empty() {
            return quote! { #variant_ident, };
        }
        let fields = v.fields.iter().map(|f| {
            let field_ident = idents::ident(&f.name);
            let field_ty = render_type_ref(&f.ty, &dt.type_name, pkg, opts);
            quote! { #field_ident: #field_ty, }
        });
        quote! { #variant_ident { #(#fields)* }, }
    });

    let ability_impls =
        ability_impls_for_datatype(dt, &type_ident, &type_params, &bounds, &type_generics, opts);

    let doc = doc_lines(&[
        format!("Move type: `{address}::{module}::{move_name}`."),
        abilities
            .as_deref()
            .map(|a| format!("Abilities: `{a}`."))
            .unwrap_or_else(|| "Abilities: *(none)*.".to_string()),
    ]);

    quote! {
        #doc
        #[derive(#derives)]
        #serde_crate_attr
        pub enum #type_ident #type_generics {
            #(#variants_tokens)*
        }

        impl #impl_generics #sm::MoveType for #type_ident #type_generics
        #where_clause
        {
            fn type_tag_static() -> #sm::__private::sui_sdk_types::TypeTag {
                #sm::__private::sui_sdk_types::TypeTag::Struct(Box::new(
                    <Self as #sm::MoveStruct>::struct_tag_static(),
                ))
            }
        }

        impl #impl_generics #sm::MoveStruct for #type_ident #type_generics
        #where_clause
        {
            fn struct_tag_static() -> #sm::__private::sui_sdk_types::StructTag {
                #struct_tag_builder
            }
        }

        #ability_impls
    }
}

fn ability_impls_for_datatype(
    dt: &Datatype,
    type_ident: &syn::Ident,
    type_params: &[syn::Ident],
    bounds: &[TokenStream],
    type_generics: &TokenStream,
    opts: &RenderOptions,
) -> TokenStream {
    let mut out = Vec::new();

    let has_key = dt.abilities.contains(&Ability::Key);
    let has_store = dt.abilities.contains(&Ability::Store);
    let has_copy = dt.abilities.contains(&Ability::Copy);
    let has_drop = dt.abilities.contains(&Ability::Drop);

    let sm = if opts.use_aliases {
        quote! { sm }
    } else {
        quote! { sui_move }
    };

    let (impl_generics, _) = impl_and_type_generics(type_params);
    let where_clause = where_clause(bounds);

    if has_key {
        out.push(quote! {
            impl #impl_generics #sm::HasKey for #type_ident #type_generics
            #where_clause
            {}
        });
    }
    if has_store {
        out.push(quote! {
            impl #impl_generics #sm::HasStore for #type_ident #type_generics
            #where_clause
            {}
        });
    }
    if has_copy {
        out.push(quote! {
            impl #impl_generics #sm::HasCopy for #type_ident #type_generics
            #where_clause
            {}
        });
    }
    if has_drop {
        out.push(quote! {
            impl #impl_generics #sm::HasDrop for #type_ident #type_generics
            #where_clause
            {}
        });
    }

    quote! { #(#out)* }
}

fn struct_tag_builder_tokens(dt: &Datatype, use_aliases: bool) -> TokenStream {
    let sm = if use_aliases {
        quote! { sm }
    } else {
        quote! { sui_move }
    };

    let address = syn::LitStr::new(&dt.type_name.address, proc_macro2::Span::call_site());
    let module = syn::LitStr::new(&dt.type_name.module, proc_macro2::Span::call_site());
    let name = syn::LitStr::new(&dt.type_name.name, proc_macro2::Span::call_site());

    let type_params = type_params_idents(dt.type_parameters.len());
    let ty_params_for_tag = type_params
        .iter()
        .map(|p| quote! { <#p as #sm::MoveType>::type_tag_static() });

    quote! {
        #sm::__private::sui_sdk_types::StructTag::new(
            #sm::parse_address(#address).expect("invalid address literal"),
            #sm::parse_identifier(#module).expect("invalid module"),
            #sm::parse_identifier(#name).expect("invalid struct name"),
            vec![#(#ty_params_for_tag),*],
        )
    }
}

fn enum_derives(abilities: &[Ability], use_aliases: bool) -> TokenStream {
    let sm = if use_aliases {
        quote! { sm }
    } else {
        quote! { sui_move }
    };

    let has_copy = abilities.contains(&Ability::Copy);

    if has_copy {
        quote! {
            ::core::clone::Clone,
            ::core::fmt::Debug,
            ::core::cmp::PartialEq,
            ::core::cmp::Eq,
            #sm::__private::serde::Serialize,
            #sm::__private::serde::Deserialize,
        }
    } else {
        quote! {
            ::core::fmt::Debug,
            ::core::cmp::PartialEq,
            ::core::cmp::Eq,
            #sm::__private::serde::Serialize,
            #sm::__private::serde::Deserialize,
        }
    }
}

fn serde_crate_attr() -> TokenStream {
    quote! { #[serde(crate = "sui_move::__private::serde")] }
}

fn type_param_bounds(dt: &Datatype, use_aliases: bool) -> Vec<TokenStream> {
    let sm = if use_aliases {
        quote! { sm }
    } else {
        quote! { sui_move }
    };

    dt.type_parameters
        .iter()
        .enumerate()
        .map(|(idx, p)| {
            let ty = format_ident!("T{idx}");
            let mut bounds: Vec<TokenStream> = vec![quote! { #sm::MoveType }];
            for a in &p.constraints {
                bounds.push(match a {
                    Ability::Copy => quote! { #sm::HasCopy },
                    Ability::Drop => quote! { #sm::HasDrop },
                    Ability::Store => quote! { #sm::HasStore },
                    Ability::Key => quote! { #sm::HasKey },
                });
            }
            quote! { #ty: #(#bounds)+* }
        })
        .collect()
}

fn where_clause(bounds: &[TokenStream]) -> TokenStream {
    if bounds.is_empty() {
        quote! {}
    } else {
        quote! { where #(#bounds,)* }
    }
}

fn render_type_ref(
    ty: &TypeRef,
    current_type: &TypeName,
    pkg: &NormalizedPackage,
    opts: &RenderOptions,
) -> TokenStream {
    match ty {
        TypeRef::Address => prelude_type(opts.use_aliases, quote! { Address }),
        TypeRef::Bool => quote! { bool },
        TypeRef::U8 => quote! { u8 },
        TypeRef::U16 => quote! { u16 },
        TypeRef::U32 => quote! { u32 },
        TypeRef::U64 => quote! { u64 },
        TypeRef::U128 => quote! { u128 },
        TypeRef::U256 => {
            // Prefer a dedicated `U256` Rust type to match Move semantics.
            prelude_type(opts.use_aliases, quote! { U256 })
        }
        TypeRef::Vector(inner) => {
            let inner = render_type_ref(inner, current_type, pkg, opts);
            quote! { Vec<#inner> }
        }
        TypeRef::Ref { mutable, inner } => {
            let inner = render_type_ref(inner, current_type, pkg, opts);
            if *mutable {
                quote! { &mut #inner }
            } else {
                quote! { &#inner }
            }
        }
        TypeRef::Datatype {
            type_name,
            type_arguments,
        } => {
            let mut args = Vec::new();
            for a in type_arguments {
                args.push(render_type_ref(a, current_type, pkg, opts));
            }

            if let Some(builtin) = builtins::map_builtin(type_name, opts.use_aliases) {
                if args.is_empty() {
                    return builtin.path;
                }
                let path = builtin.path;
                return quote! { #path<#(#args),*> };
            }

            let is_local = is_local_type(type_name, pkg);
            if !is_local {
                // Keep generation deterministic: unknown external types must be supplied by the
                // consumer (e.g. another generated package crate).
                let msg = format!(
                    "sui-move-codegen: unknown external type `{}`; generate bindings for that package too",
                    display_type_name(type_name)
                );
                let msg_lit = syn::LitStr::new(&msg, proc_macro2::Span::call_site());
                return quote! { compile_error!(#msg_lit) };
            }

            let ty_ident = idents::ident(&type_name.name);
            let base = if opts.flatten || type_name.module == current_type.module {
                quote! { #ty_ident }
            } else {
                let mod_ident = idents::ident(&type_name.module);
                quote! { super::#mod_ident::#ty_ident }
            };

            if args.is_empty() {
                base
            } else {
                quote! { #base<#(#args),*> }
            }
        }
        TypeRef::TypeParameter(idx) => {
            let ident = format_ident!("T{idx}");
            quote! { #ident }
        }
    }
}

fn prelude_type(use_aliases: bool, name: TokenStream) -> TokenStream {
    if use_aliases {
        quote! { sm::prelude::#name }
    } else {
        quote! { sui_move::prelude::#name }
    }
}

fn is_local_type(type_name: &TypeName, pkg: &NormalizedPackage) -> bool {
    if type_name.address == pkg.storage_id {
        return true;
    }
    match &pkg.original_id {
        Some(orig) => type_name.address == *orig,
        None => false,
    }
}

fn display_type_name(t: &TypeName) -> String {
    format!("{}::{}::{}", t.address, t.module, t.name)
}

fn doc_lines(lines: &[String]) -> TokenStream {
    let attrs = lines.iter().map(|line| {
        let lit = syn::LitStr::new(line, proc_macro2::Span::call_site());
        quote! { #[doc = #lit] }
    });
    quote! { #(#attrs)* }
}

fn abilities_string(abilities: &[Ability]) -> Option<String> {
    let mut out = Vec::new();
    if abilities.contains(&Ability::Key) {
        out.push("key");
    }
    if abilities.contains(&Ability::Store) {
        out.push("store");
    }
    if abilities.contains(&Ability::Copy) {
        out.push("copy");
    }
    if abilities.contains(&Ability::Drop) {
        out.push("drop");
    }
    if out.is_empty() {
        None
    } else {
        Some(out.join(", "))
    }
}

fn type_params_idents(count: usize) -> Vec<syn::Ident> {
    (0..count).map(|i| format_ident!("T{i}")).collect()
}

fn type_generics(type_params: &[syn::Ident]) -> TokenStream {
    if type_params.is_empty() {
        quote! {}
    } else {
        quote! { <#(#type_params),*> }
    }
}

fn impl_and_type_generics(type_params: &[syn::Ident]) -> (TokenStream, TokenStream) {
    let type_generics = type_generics(type_params);
    let impl_generics = if type_params.is_empty() {
        quote! {}
    } else {
        quote! { <#(#type_params),*> }
    };
    (impl_generics, type_generics)
}

fn phantom_params_string(dt: &Datatype) -> Option<String> {
    let names = dt
        .type_parameters
        .iter()
        .enumerate()
        .filter(|(_, p)| p.is_phantom)
        .map(|(idx, _)| format!("T{idx}"))
        .collect::<Vec<_>>();
    if names.is_empty() {
        None
    } else {
        Some(names.join(", "))
    }
}

fn type_abilities_string(dt: &Datatype) -> Option<String> {
    let mut parts = Vec::new();
    for (idx, p) in dt.type_parameters.iter().enumerate() {
        if p.constraints.is_empty() {
            continue;
        }
        let mut abilities = Vec::new();
        if p.constraints.contains(&Ability::Key) {
            abilities.push("key");
        }
        if p.constraints.contains(&Ability::Store) {
            abilities.push("store");
        }
        if p.constraints.contains(&Ability::Copy) {
            abilities.push("copy");
        }
        if p.constraints.contains(&Ability::Drop) {
            abilities.push("drop");
        }
        parts.push(format!("T{idx}: {}", abilities.join(", ")));
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("; "))
    }
}
