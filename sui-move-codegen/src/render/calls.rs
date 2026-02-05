//! Call-stub rendering (Move functions → `CallSpec` builders).
//!
//! Design goals:
//! - keep the generated API “honest”: it mirrors the Move signature shape (generic params, `&mut`)
//! - keep generated calls composable: every function returns a `CallSpec`
//! - keep `TxContext` out of user code: higher layers supply it during PTB building

use proc_macro2::TokenStream;
use quote::{format_ident, quote};

use crate::ir::{Ability, Function, NormalizedModule, NormalizedPackage, TypeRef};

use super::{callable, idents, types, ExternalResolver, RenderOptions};

pub(crate) fn render_functions(
    module: &NormalizedModule,
    pkg: &NormalizedPackage,
    opts: &RenderOptions,
    resolver: Option<&ExternalResolver>,
) -> Vec<TokenStream> {
    module
        .functions
        .iter()
        .filter(|f| callable::is_callable(f))
        .map(|f| render_function(module, f, pkg, opts, resolver))
        .collect()
}

fn render_function(
    module: &NormalizedModule,
    f: &Function,
    pkg: &NormalizedPackage,
    opts: &RenderOptions,
    resolver: Option<&ExternalResolver>,
) -> TokenStream {
    let sm_call = if opts.use_aliases {
        quote! { sm_call }
    } else {
        quote! { sui_move_call }
    };

    let fn_ident = idents::ident(&f.name);

    let type_params = (0..f.type_parameters.len())
        .map(|i| format_ident!("T{i}"))
        .collect::<Vec<_>>();
    let fn_generics = type_generics(&type_params);
    let bounds = type_param_bounds(f, opts.use_aliases);
    let where_clause = where_clause(&bounds);

    let module_name = syn::LitStr::new(&module.name, proc_macro2::Span::call_site());
    let function_name = syn::LitStr::new(&f.name, proc_macro2::Span::call_site());

    let package_expr = if opts.flatten {
        quote! { __sui_move_bindings::call_package() }
    } else {
        quote! { super::__sui_move_bindings::call_package() }
    };

    let push_type_args = type_params
        .iter()
        .map(|ty| quote! { spec.push_type_arg::<#ty>(); });

    let (params, pushes, skipped_tx_context) =
        render_params_and_pushes(module, f, pkg, opts, resolver);

    let signature = move_signature_string(module, f);
    let doc = doc_lines(&[
        format!("Move: `{signature}`"),
        if skipped_tx_context {
            "Note: `TxContext` is omitted; the runtime layer supplies it.".to_string()
        } else {
            String::new()
        },
        if f.is_entry {
            String::new()
        } else {
            "Note: callable as `public` (not `entry`).".to_string()
        },
    ]);

    quote! {
        #doc
        #[must_use]
        pub fn #fn_ident #fn_generics ( #(#params),* ) -> #sm_call::CallSpec
        #where_clause
        {
            let mut spec = #sm_call::CallSpec::new(#package_expr, #module_name, #function_name)
                .expect("valid Move identifiers");
            #(#push_type_args)*
            #(#pushes)*
            spec
        }
    }
}

fn render_params_and_pushes(
    module: &NormalizedModule,
    f: &Function,
    pkg: &NormalizedPackage,
    opts: &RenderOptions,
    resolver: Option<&ExternalResolver>,
) -> (Vec<TokenStream>, Vec<TokenStream>, bool) {
    let sm_call = if opts.use_aliases {
        quote! { sm_call }
    } else {
        quote! { sui_move_call }
    };

    let mut params = Vec::new();
    let mut pushes = Vec::new();
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

        let is_object = is_object_type(inner, f, pkg, opts, resolver);

        if is_object {
            let obj_ty = types::render_type_ref_in_module(inner, &module.name, pkg, opts, resolver);
            let param_ty = if ref_mutable {
                quote! { &mut impl #sm_call::ObjectArg<#obj_ty> }
            } else {
                quote! { &impl #sm_call::ObjectArg<#obj_ty> }
            };
            params.push(quote! { #arg_ident: #param_ty });
            if ref_mutable {
                pushes.push(quote! { spec.push_arg_mut(#arg_ident).expect("encode arg"); });
            } else {
                pushes.push(quote! { spec.push_arg(#arg_ident).expect("encode arg"); });
            }
        } else {
            let value_ty =
                types::render_type_ref_in_module(inner, &module.name, pkg, opts, resolver);
            params.push(quote! { #arg_ident: #value_ty });
            pushes.push(quote! { spec.push_arg(&#arg_ident).expect("encode arg"); });
        }
    }

    (params, pushes, skipped_tx_context)
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
    _opts: &RenderOptions,
    resolver: Option<&ExternalResolver>,
) -> bool {
    match ty {
        TypeRef::Datatype { type_name, .. } => {
            if let Some(resolver) = resolver {
                if let Some(is_key) = resolver.type_has_key(type_name) {
                    return is_key;
                }
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

            let has_key = p.constraints.contains(&Ability::Key);
            let has_store = p.constraints.contains(&Ability::Store);
            let has_copy = p.constraints.contains(&Ability::Copy);
            // Move rule: `copy` implies `drop`.
            let has_drop = p.constraints.contains(&Ability::Drop) || has_copy;

            let base = if has_key {
                quote! { #sm::MoveStruct }
            } else {
                quote! { #sm::MoveType }
            };

            let mut bounds: Vec<TokenStream> = vec![base];
            if has_store {
                bounds.push(quote! { #sm::HasStore });
            }
            if has_copy {
                bounds.push(quote! { #sm::HasCopy });
            }
            if has_drop {
                bounds.push(quote! { #sm::HasDrop });
            }
            if has_key {
                bounds.push(quote! { #sm::HasKey });
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
