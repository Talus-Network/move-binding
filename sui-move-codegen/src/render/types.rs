//! Datatype rendering (`struct`/`enum`) for generated bindings.
//!
//! Structs are emitted using `#[sui_move::move_struct]` so the generated code automatically
//! gets `MoveType` / `MoveStruct` implementations plus ability marker traits.
//!
//! Enums are emitted as Rust `enum`s with manual `MoveType` / `MoveStruct` impls. (Move enum
//! support is still evolving, so this layer keeps the implementation small and explicit.)

use std::collections::BTreeSet;

use proc_macro2::TokenStream;
use quote::{format_ident, quote};

use crate::ir::{Ability, Datatype, DatatypeKind, Field, NormalizedPackage, TypeName, TypeRef};

use super::{builtins, idents, ExternalType, RenderOptions};

pub(crate) fn render_datatype(
    dt: &Datatype,
    pkg: &NormalizedPackage,
    opts: &RenderOptions,
) -> TokenStream {
    let datatype = match &dt.kind {
        DatatypeKind::Struct { fields } => render_struct(dt, fields, pkg, opts),
        DatatypeKind::Enum { variants } => render_enum(dt, variants, pkg, opts),
    };
    let helpers = render_canonical_helpers(dt, pkg, opts);
    quote! {
        #datatype
        #helpers
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

pub(crate) fn render_type_ref_in_root(
    ty: &TypeRef,
    pkg: &NormalizedPackage,
    opts: &RenderOptions,
) -> TokenStream {
    render_type_ref_root(ty, pkg, opts)
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
    let address_fn = if opts.flatten {
        syn::LitStr::new("type_package", proc_macro2::Span::call_site())
    } else {
        syn::LitStr::new("super::type_package", proc_macro2::Span::call_site())
    };

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

    let macro_path = if opts.use_aliases {
        quote! { sm::move_struct }
    } else {
        quote! { sui_move::move_struct }
    };
    let rust_copy_derive = is_rust_copy_datatype(dt, pkg).then(|| {
        quote! {
            #[derive(::core::marker::Copy)]
        }
    });

    quote! {
        #doc
        #rust_copy_derive
        #[#macro_path(
            address = #address_lit,
            address_fn = #address_fn,
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
    let rust_copy = is_rust_copy_datatype(dt, pkg);

    let sm = if opts.use_aliases {
        quote! { sm }
    } else {
        quote! { sui_move }
    };
    let struct_tag_builder = struct_tag_builder_tokens(dt, opts.use_aliases);
    let where_clause = where_clause(&bounds);

    let derives = enum_derives(&dt.abilities, rust_copy, opts.use_aliases);
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

fn render_canonical_helpers(
    dt: &Datatype,
    pkg: &NormalizedPackage,
    opts: &RenderOptions,
) -> TokenStream {
    if !matches!(dt.kind, DatatypeKind::Struct { .. }) {
        return quote! {};
    }

    let mut helpers = Vec::new();
    if let Some(helper) = render_struct_constructor_helpers(dt, pkg, opts) {
        helpers.push(helper);
    }
    if is_type(&dt.type_name, "0x1", "ascii", "String") {
        helpers.push(render_ascii_string_helpers(dt));
    }
    if is_type(&dt.type_name, "0x1", "type_name", "TypeName") {
        helpers.push(render_type_name_helpers(dt, pkg, opts));
    } else if let Some(helper) = render_ascii_name_wrapper_helpers(dt, pkg, opts) {
        helpers.push(helper);
    }
    if is_type(&dt.type_name, "0x1", "option", "Option") {
        helpers.push(render_option_helpers(dt, opts));
    }
    if is_type(&dt.type_name, "0x2", "object", "ID") {
        helpers.push(render_object_id_helpers(dt));
    }
    if is_type(&dt.type_name, "0x2", "object", "UID") {
        helpers.push(render_object_uid_helpers(dt));
    }
    if is_type(&dt.type_name, "0x2", "table", "Table") {
        helpers.push(render_table_like_helpers(
            dt,
            opts,
            quote! {
                Self {
                    id: super::object::UID::new(id),
                    size,
                    phantom_t0: std::marker::PhantomData,
                    phantom_t1: std::marker::PhantomData,
                }
            },
        ));
    }
    if is_type(&dt.type_name, "0x2", "linked_table", "LinkedTable") {
        helpers.push(render_table_like_helpers(
            dt,
            opts,
            quote! {
                Self {
                    id: super::object::UID::new(id),
                    size,
                    head: Default::default(),
                    tail: Default::default(),
                    phantom_t1: std::marker::PhantomData,
                }
            },
        ));
    }
    if is_type(&dt.type_name, "0x2", "object_table", "ObjectTable") {
        helpers.push(render_id_size_helpers(dt, opts));
    }
    if is_type(&dt.type_name, "0x2", "bag", "Bag")
        || is_type(&dt.type_name, "0x2", "object_bag", "ObjectBag")
    {
        helpers.push(render_table_like_helpers(
            dt,
            opts,
            quote! {
                Self {
                    id: super::object::UID::new(id),
                    size,
                }
            },
        ));
    }
    if is_type(&dt.type_name, "0x2", "table_vec", "TableVec") {
        helpers.push(render_table_vec_helpers(dt, opts));
    }
    if is_type(&dt.type_name, "0x2", "vec_map", "VecMap") {
        helpers.push(render_vec_map_helpers(dt, opts));
    }

    quote! { #(#helpers)* }
}

fn is_rust_copy_datatype(dt: &Datatype, pkg: &NormalizedPackage) -> bool {
    rust_copy_type_keys(pkg).contains(&local_type_key(&dt.type_name, pkg))
}

fn rust_copy_type_keys(pkg: &NormalizedPackage) -> BTreeSet<String> {
    let datatypes = pkg
        .modules
        .values()
        .flat_map(|module| module.datatypes.iter())
        .collect::<Vec<_>>();
    let mut copy_types = BTreeSet::new();

    loop {
        let mut changed = false;
        for dt in &datatypes {
            let key = local_type_key(&dt.type_name, pkg);
            if copy_types.contains(&key) {
                continue;
            }
            if datatype_has_rust_copy_shape(dt, pkg, &copy_types) {
                copy_types.insert(key);
                changed = true;
            }
        }

        if !changed {
            return copy_types;
        }
    }
}

fn datatype_has_rust_copy_shape(
    dt: &Datatype,
    pkg: &NormalizedPackage,
    copy_types: &BTreeSet<String>,
) -> bool {
    if !dt.abilities.contains(&Ability::Copy) || !dt.type_parameters.is_empty() {
        return false;
    }

    match &dt.kind {
        DatatypeKind::Struct { fields } => fields
            .iter()
            .all(|field| type_has_rust_copy_shape(&field.ty, pkg, copy_types)),
        DatatypeKind::Enum { variants } => variants.iter().all(|variant| {
            variant
                .fields
                .iter()
                .all(|field| type_has_rust_copy_shape(&field.ty, pkg, copy_types))
        }),
    }
}

fn type_has_rust_copy_shape(
    ty: &TypeRef,
    pkg: &NormalizedPackage,
    copy_types: &BTreeSet<String>,
) -> bool {
    match ty {
        TypeRef::Address
        | TypeRef::Bool
        | TypeRef::U8
        | TypeRef::U16
        | TypeRef::U32
        | TypeRef::U64
        | TypeRef::U128
        | TypeRef::U256 => true,
        TypeRef::Ref { mutable, .. } => !mutable,
        TypeRef::Datatype {
            type_name,
            type_arguments,
        } => {
            type_arguments.is_empty()
                && is_local_type(type_name, pkg)
                && copy_types.contains(&local_type_key(type_name, pkg))
        }
        TypeRef::Vector(_) | TypeRef::TypeParameter(_) => false,
    }
}

fn local_type_key(type_name: &TypeName, pkg: &NormalizedPackage) -> String {
    let address = if is_local_type(type_name, pkg) {
        normalize_address(&pkg.storage_id)
    } else {
        normalize_address(&type_name.address)
    };
    format!("{}::{}::{}", address, type_name.module, type_name.name)
}

fn render_struct_constructor_helpers(
    dt: &Datatype,
    pkg: &NormalizedPackage,
    opts: &RenderOptions,
) -> Option<TokenStream> {
    if has_specialized_new_helper(dt, pkg, opts) {
        return None;
    }

    let DatatypeKind::Struct { fields } = &dt.kind else {
        return None;
    };

    let type_ident = idents::ident(&dt.name);
    let type_params = type_params_idents(dt.type_parameters.len());
    let (impl_generics, type_generics) = impl_and_type_generics(&type_params);

    let mut params = Vec::new();
    let mut initializers = Vec::new();
    let field_names = fields
        .iter()
        .map(|field| field.name.as_str())
        .collect::<Vec<_>>();
    for field in fields {
        let field_ident = idents::ident(&field.name);
        if field.name.starts_with("phantom_") {
            initializers.push(quote! { #field_ident: std::marker::PhantomData });
            continue;
        }

        let field_ty = render_type_ref(&field.ty, &dt.type_name, pkg, opts);
        let param_ty = constructor_param_type(&field.ty, field_ty);
        params.push(quote! { #field_ident: #param_ty });
        initializers.push(quote! { #field_ident: #field_ident.into() });
    }
    for (idx, type_param) in dt.type_parameters.iter().enumerate() {
        let field_name = format!("phantom_t{idx}");
        if type_param.is_phantom && !field_names.contains(&field_name.as_str()) {
            let field_ident = format_ident!("phantom_t{idx}");
            initializers.push(quote! { #field_ident: std::marker::PhantomData });
        }
    }

    Some(quote! {
        impl #impl_generics #type_ident #type_generics
        {
            pub fn new(#(#params),*) -> Self {
                Self {
                    #(#initializers),*
                }
            }
        }
    })
}

fn constructor_param_type(ty: &TypeRef, rendered: TokenStream) -> TokenStream {
    match ty {
        TypeRef::Datatype { .. } | TypeRef::TypeParameter(_) => quote! { impl Into<#rendered> },
        TypeRef::Address
        | TypeRef::Bool
        | TypeRef::U8
        | TypeRef::U16
        | TypeRef::U32
        | TypeRef::U64
        | TypeRef::U128
        | TypeRef::U256
        | TypeRef::Vector(_)
        | TypeRef::Ref { .. } => rendered,
    }
}

fn has_specialized_new_helper(
    dt: &Datatype,
    pkg: &NormalizedPackage,
    opts: &RenderOptions,
) -> bool {
    is_type(&dt.type_name, "0x1", "type_name", "TypeName")
        || ascii_name_field(dt, pkg, opts).is_some()
        || is_type(&dt.type_name, "0x2", "object", "ID")
        || is_type(&dt.type_name, "0x2", "object", "UID")
        || is_type(&dt.type_name, "0x2", "table", "Table")
        || is_type(&dt.type_name, "0x2", "linked_table", "LinkedTable")
        || is_type(&dt.type_name, "0x2", "bag", "Bag")
        || is_type(&dt.type_name, "0x2", "object_bag", "ObjectBag")
        || is_type(&dt.type_name, "0x2", "table_vec", "TableVec")
}

fn is_type(type_name: &TypeName, address: &str, module: &str, name: &str) -> bool {
    normalize_address(&type_name.address) == normalize_address(address)
        && type_name.module == module
        && type_name.name == name
}

fn normalize_address(address: &str) -> String {
    let trimmed = address.trim_start_matches("0x");
    let trimmed = trimmed.trim_start_matches('0');
    if trimmed.is_empty() {
        "0".to_string()
    } else {
        trimmed.to_ascii_lowercase()
    }
}

fn render_ascii_string_helpers(dt: &Datatype) -> TokenStream {
    let type_ident = idents::ident(&dt.name);
    quote! {
        impl #type_ident {
            pub fn as_str(&self) -> &str {
                std::str::from_utf8(&self.bytes).expect("generated Move ASCII string must be UTF-8")
            }

            pub fn into_string(self) -> std::string::String {
                std::string::String::from_utf8(self.bytes)
                    .expect("generated Move ASCII string must be UTF-8")
            }
        }

        impl From<&str> for #type_ident {
            fn from(value: &str) -> Self {
                Self {
                    bytes: value.as_bytes().to_vec(),
                }
            }
        }

        impl From<std::string::String> for #type_ident {
            fn from(value: std::string::String) -> Self {
                Self {
                    bytes: value.into_bytes(),
                }
            }
        }

        impl From<#type_ident> for std::string::String {
            fn from(value: #type_ident) -> Self {
                value.into_string()
            }
        }

        impl AsRef<str> for #type_ident {
            fn as_ref(&self) -> &str {
                self.as_str()
            }
        }

        impl std::fmt::Display for #type_ident {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.as_str())
            }
        }
    }
}

fn render_type_name_helpers(
    dt: &Datatype,
    pkg: &NormalizedPackage,
    opts: &RenderOptions,
) -> TokenStream {
    let type_ident = idents::ident(&dt.name);
    let Some((field_ident, field_ty)) = ascii_name_field(dt, pkg, opts) else {
        return quote! {};
    };

    quote! {
        impl #type_ident {
            pub fn new(name: &str) -> Self {
                Self {
                    #field_ident: #field_ty::from(name),
                }
            }

            pub fn as_str(&self) -> &str {
                self.#field_ident.as_str()
            }

            fn normalize(name: &str) -> std::borrow::Cow<'_, str> {
                let trimmed = name.trim_start_matches("0x");
                if trimmed.len() == name.len() {
                    std::borrow::Cow::Borrowed(name)
                } else {
                    std::borrow::Cow::Owned(trimmed.to_string())
                }
            }

            pub fn matches_qualified_name(&self, expected: &str) -> bool {
                Self::normalize(self.as_str()).eq_ignore_ascii_case(&Self::normalize(expected))
            }
        }

        impl From<&str> for #type_ident {
            fn from(name: &str) -> Self {
                #type_ident::new(name)
            }
        }

        impl From<std::string::String> for #type_ident {
            fn from(name: std::string::String) -> Self {
                #type_ident {
                    #field_ident: #field_ty::from(name),
                }
            }
        }

        impl std::fmt::Display for #type_ident {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                self.#field_ident.fmt(f)
            }
        }
    }
}

fn render_ascii_name_wrapper_helpers(
    dt: &Datatype,
    pkg: &NormalizedPackage,
    opts: &RenderOptions,
) -> Option<TokenStream> {
    let type_ident = idents::ident(&dt.name);
    let (field_ident, field_ty) = ascii_name_field(dt, pkg, opts)?;

    Some(quote! {
        impl #type_ident {
            pub fn new(name: impl Into<#field_ty>) -> Self {
                Self { #field_ident: name.into() }
            }

            pub fn as_str(&self) -> &str {
                self.#field_ident.as_str()
            }
        }

        impl From<&str> for #type_ident {
            fn from(value: &str) -> Self {
                Self::new(value)
            }
        }

        impl From<std::string::String> for #type_ident {
            fn from(value: std::string::String) -> Self {
                Self::new(value)
            }
        }
    })
}

fn ascii_name_field(
    dt: &Datatype,
    pkg: &NormalizedPackage,
    opts: &RenderOptions,
) -> Option<(syn::Ident, TokenStream)> {
    let DatatypeKind::Struct { fields } = &dt.kind else {
        return None;
    };
    let [field] = fields.as_slice() else {
        return None;
    };
    if field.name != "name" || !is_ascii_string_ref(&field.ty) {
        return None;
    }
    let field_ident = idents::ident(&field.name);
    let field_ty = render_type_ref(&field.ty, &dt.type_name, pkg, opts);
    Some((field_ident, field_ty))
}

fn is_ascii_string_ref(ty: &TypeRef) -> bool {
    matches!(
        ty,
        TypeRef::Datatype {
            type_name,
            type_arguments
        } if type_arguments.is_empty() && is_type(type_name, "0x1", "ascii", "String")
    )
}

fn render_option_helpers(dt: &Datatype, _opts: &RenderOptions) -> TokenStream {
    let type_ident = idents::ident(&dt.name);
    let type_params = type_params_idents(dt.type_parameters.len());
    if type_params.len() != 1 {
        return quote! {};
    }
    let t0 = &type_params[0];
    let (impl_generics, type_generics) = impl_and_type_generics(&type_params);

    quote! {
        impl #impl_generics #type_ident #type_generics
        {
            pub fn from_option(value: std::option::Option<#t0>) -> Self {
                Self {
                    vec: value.into_iter().collect(),
                }
            }

            pub fn into_option(self) -> std::option::Option<#t0> {
                self.vec.into_iter().next()
            }

            pub fn as_option(&self) -> std::option::Option<&#t0> {
                self.vec.first()
            }

            pub fn copied_option(&self) -> std::option::Option<#t0>
            where
                #t0: Copy,
            {
                self.as_option().copied()
            }

            pub fn cloned_option(&self) -> std::option::Option<#t0>
            where
                #t0: Clone,
            {
                self.as_option().cloned()
            }
        }

        impl #impl_generics Default for #type_ident #type_generics
        {
            fn default() -> Self {
                Self::from_option(None)
            }
        }

        impl #impl_generics From<std::option::Option<#t0>> for #type_ident #type_generics
        {
            fn from(value: std::option::Option<#t0>) -> Self {
                Self::from_option(value)
            }
        }
    }
}

fn render_object_id_helpers(dt: &Datatype) -> TokenStream {
    let type_ident = idents::ident(&dt.name);
    quote! {
        impl #type_ident {
            pub fn new(bytes: sui_move::prelude::Address) -> Self {
                Self { bytes }
            }

            pub fn address(&self) -> sui_move::prelude::Address {
                self.bytes
            }
        }

        impl From<sui_move::prelude::Address> for #type_ident {
            fn from(value: sui_move::prelude::Address) -> Self {
                Self::new(value)
            }
        }

        impl From<#type_ident> for sui_move::prelude::Address {
            fn from(value: #type_ident) -> Self {
                value.bytes
            }
        }

        impl std::fmt::Display for #type_ident {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                self.bytes.fmt(f)
            }
        }
    }
}

fn render_object_uid_helpers(dt: &Datatype) -> TokenStream {
    let type_ident = idents::ident(&dt.name);
    quote! {
        impl #type_ident {
            pub fn new(bytes: sui_move::prelude::Address) -> Self {
                Self {
                    id: ID::new(bytes),
                }
            }

            pub fn address(&self) -> sui_move::prelude::Address {
                self.id.bytes
            }
        }

        impl From<sui_move::prelude::Address> for #type_ident {
            fn from(value: sui_move::prelude::Address) -> Self {
                Self::new(value)
            }
        }

        impl From<#type_ident> for sui_move::prelude::Address {
            fn from(value: #type_ident) -> Self {
                value.id.bytes
            }
        }
    }
}

fn render_table_like_helpers(
    dt: &Datatype,
    _opts: &RenderOptions,
    constructor: TokenStream,
) -> TokenStream {
    let type_ident = idents::ident(&dt.name);
    let type_params = type_params_idents(dt.type_parameters.len());
    let (impl_generics, type_generics) = impl_and_type_generics(&type_params);

    quote! {
        impl #impl_generics #type_ident #type_generics
        {
            pub fn new(id: sui_move::prelude::Address, size: u64) -> Self {
                #constructor
            }

            pub fn id(&self) -> sui_move::prelude::Address {
                self.id.id.bytes
            }

            pub fn size(&self) -> usize {
                usize::try_from(self.size).unwrap_or(usize::MAX)
            }

            pub fn size_u64(&self) -> u64 {
                self.size
            }
        }
    }
}

fn render_id_size_helpers(dt: &Datatype, _opts: &RenderOptions) -> TokenStream {
    let type_ident = idents::ident(&dt.name);
    let type_params = type_params_idents(dt.type_parameters.len());
    let (impl_generics, type_generics) = impl_and_type_generics(&type_params);

    quote! {
        impl #impl_generics #type_ident #type_generics
        {
            pub fn id(&self) -> sui_move::prelude::Address {
                self.id.id.bytes
            }

            pub fn size(&self) -> usize {
                usize::try_from(self.size).unwrap_or(usize::MAX)
            }

            pub fn size_u64(&self) -> u64 {
                self.size
            }
        }
    }
}

fn render_table_vec_helpers(dt: &Datatype, _opts: &RenderOptions) -> TokenStream {
    let type_ident = idents::ident(&dt.name);
    let type_params = type_params_idents(dt.type_parameters.len());
    let (impl_generics, type_generics) = impl_and_type_generics(&type_params);

    quote! {
        impl #impl_generics #type_ident #type_generics
        {
            pub fn new(id: sui_move::prelude::Address, size: u64) -> Self {
                Self {
                    contents: super::table::Table::new(id, size),
                    phantom_t0: std::marker::PhantomData,
                }
            }

            pub fn id(&self) -> sui_move::prelude::Address {
                self.contents.id()
            }

            pub fn size(&self) -> usize {
                self.contents.size()
            }

            pub fn size_u64(&self) -> u64 {
                self.contents.size_u64()
            }
        }
    }
}

fn render_vec_map_helpers(dt: &Datatype, _opts: &RenderOptions) -> TokenStream {
    let type_ident = idents::ident(&dt.name);
    let type_params = type_params_idents(dt.type_parameters.len());
    if type_params.len() != 2 {
        return quote! {};
    }
    let k = &type_params[0];
    let v = &type_params[1];
    let (impl_generics, type_generics) = impl_and_type_generics(&type_params);

    quote! {
        impl #impl_generics #type_ident #type_generics
        {
            pub fn get(&self, key: &#k) -> std::option::Option<&#v>
            where
                #k: Eq,
            {
                self.contents
                    .iter()
                    .find(|entry| &entry.key == key)
                    .map(|entry| &entry.value)
            }

            pub fn into_hash_map(self) -> std::collections::HashMap<#k, #v>
            where
                #k: Eq + std::hash::Hash,
            {
                self.contents
                    .into_iter()
                    .map(|entry| (entry.key, entry.value))
                    .collect()
            }
        }
    }
}

fn struct_tag_builder_tokens(dt: &Datatype, use_aliases: bool) -> TokenStream {
    let sm = if use_aliases {
        quote! { sm }
    } else {
        quote! { sui_move }
    };

    let module = syn::LitStr::new(&dt.type_name.module, proc_macro2::Span::call_site());
    let name = syn::LitStr::new(&dt.type_name.name, proc_macro2::Span::call_site());

    let type_params = type_params_idents(dt.type_parameters.len());
    let ty_params_for_tag = type_params
        .iter()
        .map(|p| quote! { <#p as #sm::MoveType>::type_tag_static() });

    quote! {
        #sm::__private::sui_sdk_types::StructTag::new(
            type_package(),
            #sm::parse_identifier(#module).expect("invalid module"),
            #sm::parse_identifier(#name).expect("invalid struct name"),
            vec![#(#ty_params_for_tag),*],
        )
    }
}

fn enum_derives(abilities: &[Ability], rust_copy: bool, use_aliases: bool) -> TokenStream {
    let sm = if use_aliases {
        quote! { sm }
    } else {
        quote! { sui_move }
    };

    let has_copy = abilities.contains(&Ability::Copy);
    let rust_copy_derive = rust_copy.then(|| quote! { ::core::marker::Copy, });

    if has_copy {
        quote! {
            #rust_copy_derive
            ::core::clone::Clone,
            ::core::fmt::Debug,
            ::core::cmp::PartialEq,
            ::core::cmp::Eq,
            ::core::hash::Hash,
            #sm::__private::serde::Serialize,
            #sm::__private::serde::Deserialize,
        }
    } else {
        quote! {
            ::core::fmt::Debug,
            ::core::cmp::PartialEq,
            ::core::cmp::Eq,
            ::core::hash::Hash,
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
                if let Some(external) = opts.external_types.get(type_name) {
                    return render_external_type(external, &args);
                }
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

fn render_type_ref_root(
    ty: &TypeRef,
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
        TypeRef::U256 => prelude_type(opts.use_aliases, quote! { U256 }),
        TypeRef::Vector(inner) => {
            let inner = render_type_ref_root(inner, pkg, opts);
            quote! { Vec<#inner> }
        }
        TypeRef::Ref { mutable, inner } => {
            let inner = render_type_ref_root(inner, pkg, opts);
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
                args.push(render_type_ref_root(a, pkg, opts));
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
                if let Some(external) = opts.external_types.get(type_name) {
                    return render_external_type(external, &args);
                }
                let msg = format!(
                    "sui-move-codegen: unknown external type `{}`; generate bindings for that package too",
                    display_type_name(type_name)
                );
                let msg_lit = syn::LitStr::new(&msg, proc_macro2::Span::call_site());
                return quote! { compile_error!(#msg_lit) };
            }

            let ty_ident = idents::ident(&type_name.name);
            let base = if opts.flatten {
                quote! { #ty_ident }
            } else {
                let mod_ident = idents::ident(&type_name.module);
                quote! { #mod_ident::#ty_ident }
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

fn render_external_type(external: &ExternalType, args: &[TokenStream]) -> TokenStream {
    let path = match syn::parse_str::<syn::Path>(&external.rust_path) {
        Ok(path) => quote! { #path },
        Err(_) => {
            let msg = format!(
                "sui-move-codegen: invalid external Rust type path `{}`",
                external.rust_path
            );
            let msg_lit = syn::LitStr::new(&msg, proc_macro2::Span::call_site());
            return quote! { compile_error!(#msg_lit) };
        }
    };

    if args.is_empty() {
        path
    } else {
        quote! { #path<#(#args),*> }
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
    let address = normalize_address(&type_name.address);
    if address == normalize_address(&pkg.storage_id) {
        return true;
    }
    match &pkg.original_id {
        Some(orig) => address == normalize_address(orig),
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
