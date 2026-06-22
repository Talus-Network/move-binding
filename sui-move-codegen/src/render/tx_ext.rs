//! Optional runtime helpers: generate a `TxExt` trait implemented for `sui-move-runtime`.
//!
//! This layer is intentionally thin: the generated methods only append a `MoveCall` command by
//! calling `Tx::call(module::function(...))`. Committing/simulating/inspecting stays explicit.

use proc_macro2::TokenStream;
use quote::{format_ident, quote};

use crate::ir::{Ability, Function, NormalizedModule, NormalizedPackage, TypeRef, Visibility};

use super::{builtins, idents, types, RenderOptions};

pub(crate) fn render_tx_ext(pkg: &NormalizedPackage, opts: &RenderOptions) -> TokenStream {
    if !opts.emit_calls {
        // Without call builders, the extension methods would need to inline call construction,
        // which defeats the layering goal.
        return quote! {};
    }

    let mut trait_methods = Vec::new();
    let mut impl_methods = Vec::new();

    for module in pkg.modules.values() {
        for f in module
            .functions
            .iter()
            .filter(|f| matches!(f.visibility, Visibility::Public))
        {
            let (trait_method, impl_method) = render_method(module, f, pkg, opts);
            trait_methods.push(trait_method);
            impl_methods.push(impl_method);
        }
    }

    if trait_methods.is_empty() {
        return quote! {};
    }

    let doc = doc_lines(&[
        "Generated `sui-move-runtime` helpers for this package.".to_string(),
        String::new(),
        "Each method appends a `MoveCall` command by calling `Tx::call(...)`.".to_string(),
        "Committing the transaction is still explicit: call `tx.commit().await`.".to_string(),
    ]);

    quote! {
        #doc
        pub trait TxExt {
            #(#trait_methods)*
        }

        impl<'a, S> TxExt for sui_move_runtime::Tx<'a, S>
        where
            S: sui_move_runtime::SuiSigner,
        {
            #(#impl_methods)*
        }
    }
}

fn render_method(
    module: &NormalizedModule,
    f: &Function,
    pkg: &NormalizedPackage,
    opts: &RenderOptions,
) -> (TokenStream, TokenStream) {
    let tx_method_ident = idents::ident(&format!("{}__{}", module.name, f.name));
    let call_fn_ident = idents::ident(&f.name);
    let call_fn_path = if opts.flatten {
        quote! { #call_fn_ident }
    } else {
        let module_ident = idents::ident(&module.name);
        quote! { #module_ident::#call_fn_ident }
    };

    let type_params = (0..f.type_parameters.len())
        .map(|i| format_ident!("T{i}"))
        .collect::<Vec<_>>();
    let fn_generics = type_generics(&type_params);
    let bounds = type_param_bounds(f, opts.use_aliases);
    let where_clause = where_clause(&bounds);

    let (params, args, skipped_tx_context) = render_params_and_args(f, pkg, opts);

    let signature = move_signature_string(module, f);
    let doc = doc_lines(&[
        format!("Move: `{signature}`"),
        if skipped_tx_context {
            "Note: `TxContext` is omitted; the runtime supplies it.".to_string()
        } else {
            String::new()
        },
    ]);

    let call_expr = if type_params.is_empty() {
        quote! { #call_fn_path( #(#args),* ) }
    } else {
        quote! { #call_fn_path::<#(#type_params),*>( #(#args),* ) }
    };

    let signature = quote! {
        #doc
        fn #tx_method_ident #fn_generics (&mut self, #(#params),*)
            -> Result<sui_sdk_types::Argument, sui_move_runtime::Error>
            #where_clause
        ;
    };

    let implementation = quote! {
        fn #tx_method_ident #fn_generics (&mut self, #(#params),*)
            -> Result<sui_sdk_types::Argument, sui_move_runtime::Error>
            #where_clause
        {
            self.call(#call_expr)
        }
    };

    (signature, implementation)
}

fn render_params_and_args(
    f: &Function,
    pkg: &NormalizedPackage,
    opts: &RenderOptions,
) -> (Vec<TokenStream>, Vec<TokenStream>, bool) {
    let sm_call = if opts.use_aliases {
        quote! { sm_call }
    } else {
        quote! { sui_move_call }
    };

    let mut params = Vec::new();
    let mut args = Vec::new();
    let mut skipped_tx_context = false;
    let mut arg_idx: usize = 0;

    for p in &f.parameters {
        if is_tx_context(&p.ty) {
            skipped_tx_context = true;
            continue;
        }

        let arg_ident = format_ident!("arg{arg_idx}");
        arg_idx += 1;
        let (ref_mutable, inner) = split_ref(&p.ty);

        let is_object = is_object_type(inner, f, pkg, opts);
        if is_object {
            let obj_ty = types::render_type_ref_in_root(inner, pkg, opts);
            let param_ty = if ref_mutable {
                quote! { &mut impl #sm_call::ObjectArg<#obj_ty> }
            } else {
                quote! { &impl #sm_call::ObjectArg<#obj_ty> }
            };
            params.push(quote! { #arg_ident: #param_ty });
        } else {
            let value_ty = types::render_type_ref_in_root(inner, pkg, opts);
            params.push(quote! { #arg_ident: #value_ty });
        }

        args.push(quote! { #arg_ident });
    }

    (params, args, skipped_tx_context)
}

fn is_tx_context(ty: &TypeRef) -> bool {
    match ty {
        TypeRef::Ref { inner, .. } => is_tx_context(inner),
        TypeRef::Datatype { type_name, .. } => {
            type_name.address == "0x2"
                && type_name.module == "tx_context"
                && type_name.name == "TxContext"
        }
        _ => false,
    }
}

fn split_ref(ty: &TypeRef) -> (bool, &TypeRef) {
    match ty {
        TypeRef::Ref { mutable, inner } => (*mutable, inner.as_ref()),
        other => (false, other),
    }
}

fn is_object_type(
    ty: &TypeRef,
    f: &Function,
    pkg: &NormalizedPackage,
    opts: &RenderOptions,
) -> bool {
    match ty {
        TypeRef::Datatype { type_name, .. } => {
            if let Some(builtin) = builtins::map_builtin(type_name, opts.use_aliases) {
                return builtin.is_key;
            }
            if let Some(external) = opts.external_types.get(type_name) {
                return external.is_key;
            }
            pkg.modules
                .get(&type_name.module)
                .and_then(|m| {
                    m.datatypes
                        .iter()
                        .find(|dt| dt.type_name == *type_name)
                        .map(|dt| dt.abilities.contains(&Ability::Key))
                })
                .unwrap_or(false)
        }
        TypeRef::TypeParameter(idx) => f
            .type_parameters
            .get(*idx as usize)
            .map(|tp| tp.constraints.contains(&Ability::Key))
            .unwrap_or(false),
        _ => false,
    }
}

fn type_param_bounds(f: &Function, use_aliases: bool) -> Vec<TokenStream> {
    let sm = if use_aliases {
        quote! { sm }
    } else {
        quote! { sui_move }
    };

    f.type_parameters
        .iter()
        .enumerate()
        .map(|(idx, p)| {
            let ty = format_ident!("T{idx}");

            let base = if p.constraints.contains(&Ability::Key) {
                quote! { #sm::MoveStruct }
            } else {
                quote! { #sm::MoveType }
            };

            let mut bounds: Vec<TokenStream> = vec![base];
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

fn type_generics(type_params: &[syn::Ident]) -> TokenStream {
    if type_params.is_empty() {
        return quote! {};
    }
    quote! { <#(#type_params),*> }
}

fn doc_lines(lines: &[String]) -> TokenStream {
    let attrs = lines.iter().filter(|l| !l.is_empty()).map(|line| {
        let lit = syn::LitStr::new(line, proc_macro2::Span::call_site());
        quote! { #[doc = #lit] }
    });
    quote! { #(#attrs)* }
}

fn move_signature_string(module: &NormalizedModule, f: &Function) -> String {
    let visibility = "public";
    let entry = if f.is_entry { " entry" } else { "" };

    let type_params = if f.type_parameters.is_empty() {
        String::new()
    } else {
        let parts = f
            .type_parameters
            .iter()
            .enumerate()
            .map(|(idx, p)| {
                let name = format!("T{idx}");
                if p.constraints.is_empty() {
                    return name;
                }
                let cons = ability_list(&p.constraints).join(" + ");
                format!("{name}: {cons}")
            })
            .collect::<Vec<_>>();
        format!("<{}>", parts.join(", "))
    };

    let params = f
        .parameters
        .iter()
        .enumerate()
        .map(|(idx, p)| format!("arg{idx}: {}", move_type_string(&p.ty)))
        .collect::<Vec<_>>()
        .join(", ");

    let returns = if f.return_types.is_empty() {
        String::new()
    } else if f.return_types.len() == 1 {
        format!(": {}", move_type_string(&f.return_types[0]))
    } else {
        let r = f
            .return_types
            .iter()
            .map(move_type_string)
            .collect::<Vec<_>>()
            .join(", ");
        format!(": ({r})")
    };

    format!(
        "{visibility}{entry} fun {}::{}{}({params}){returns}",
        module.name, f.name, type_params
    )
}

fn move_type_string(ty: &TypeRef) -> String {
    match ty {
        TypeRef::Address => "address".into(),
        TypeRef::Bool => "bool".into(),
        TypeRef::U8 => "u8".into(),
        TypeRef::U16 => "u16".into(),
        TypeRef::U32 => "u32".into(),
        TypeRef::U64 => "u64".into(),
        TypeRef::U128 => "u128".into(),
        TypeRef::U256 => "u256".into(),
        TypeRef::Vector(inner) => format!("vector<{}>", move_type_string(inner)),
        TypeRef::Ref { mutable, inner } => {
            if *mutable {
                format!("&mut {}", move_type_string(inner))
            } else {
                format!("&{}", move_type_string(inner))
            }
        }
        TypeRef::Datatype {
            type_name,
            type_arguments,
        } => {
            let base = format!(
                "{}::{}::{}",
                type_name.address, type_name.module, type_name.name
            );
            if type_arguments.is_empty() {
                base
            } else {
                let args = type_arguments
                    .iter()
                    .map(move_type_string)
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{base}<{args}>")
            }
        }
        TypeRef::TypeParameter(idx) => format!("T{idx}"),
    }
}

fn ability_list(abilities: &[Ability]) -> Vec<&'static str> {
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
    out
}
