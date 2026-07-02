//! Call-stub rendering (Move functions → call targets and optional `CallSpec` builders).
//!
//! Design goals:
//! - keep the generated API “honest”: it mirrors the Move signature shape (generic params, `&mut`)
//! - keep generated calls composable: every function exposes a `CallTarget`
//! - optionally mirror the Move signature as a typed `CallSpec` builder
//! - keep `TxContext` out of user code: higher layers supply it during PTB building

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use std::collections::{BTreeMap, BTreeSet};

use crate::ir::{
    Ability, Datatype, DatatypeKind, Field, Function, NormalizedModule, NormalizedPackage,
    TypeName, TypeRef, Visibility,
};

use super::{builtins, idents, types, RenderOptions};

pub(crate) fn render_functions(
    module: &NormalizedModule,
    pkg: &NormalizedPackage,
    opts: &RenderOptions,
) -> Vec<TokenStream> {
    module
        .functions
        .iter()
        .filter(|f| matches!(f.visibility, Visibility::Public) || f.is_entry)
        .map(|f| render_function(module, f, pkg, opts))
        .collect()
}

fn render_function(
    module: &NormalizedModule,
    f: &Function,
    pkg: &NormalizedPackage,
    opts: &RenderOptions,
) -> TokenStream {
    let sm_call = if opts.use_aliases {
        quote! { sm_call }
    } else {
        quote! { sui_move_call }
    };

    let fn_ident = idents::ident(&f.name);
    let target_fn_ident = idents::ident(&format!("{}_target", f.name));

    let type_params = (0..f.type_parameters.len())
        .map(|i| format_ident!("T{i}"))
        .collect::<Vec<_>>();
    let fn_generics = type_generics(&type_params);
    let bounds = type_param_bounds(f, pkg, opts.use_aliases);
    let where_clause = where_clause(&bounds);

    let module_name = syn::LitStr::new(&module.name, proc_macro2::Span::call_site());
    let function_name = syn::LitStr::new(&f.name, proc_macro2::Span::call_site());

    let target_init = if type_params.is_empty() {
        quote! { let target = #sm_call::CallTarget::new(call_package(), #module_name, #function_name)?; }
    } else {
        quote! { let mut target = #sm_call::CallTarget::new(call_package(), #module_name, #function_name)?; }
    };

    let push_type_args = type_params
        .iter()
        .map(|ty| quote! { target.push_type_arg::<#ty>(); });

    let skipped_tx_context = has_tx_context(f);
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
            "Note: this function is not marked `entry`.".to_string()
        },
    ]);
    let target_call = if type_params.is_empty() {
        quote! { #target_fn_ident() }
    } else {
        quote! { #target_fn_ident::<#(#type_params),*>() }
    };
    let spec_builder = if opts.emit_call_specs {
        let (params, pushes) = render_params_and_pushes(module, f, pkg, opts);
        let spec_init = if pushes.is_empty() {
            quote! { let spec = #sm_call::CallSpec::from_target(#target_call?); }
        } else {
            quote! { let mut spec = #sm_call::CallSpec::from_target(#target_call?); }
        };

        quote! {
            #doc
            pub fn #fn_ident #fn_generics ( #(#params),* ) -> Result<#sm_call::CallSpec, #sm_call::CallSpecError>
            #where_clause
            {
                #spec_init
                #(#pushes)*
                Ok(spec)
            }
        }
    } else {
        quote! {}
    };

    quote! {
        #doc
        pub fn #target_fn_ident #fn_generics () -> Result<#sm_call::CallTarget, #sm_call::CallSpecError>
        #where_clause
        {
            #target_init
            #(#push_type_args)*
            Ok(target)
        }

        #spec_builder
    }
}

fn render_params_and_pushes(
    module: &NormalizedModule,
    f: &Function,
    pkg: &NormalizedPackage,
    opts: &RenderOptions,
) -> (Vec<TokenStream>, Vec<TokenStream>) {
    let sm_call = if opts.use_aliases {
        quote! { sm_call }
    } else {
        quote! { sui_move_call }
    };

    let mut params = Vec::new();
    let mut pushes = Vec::new();
    let mut arg_idx: usize = 0;

    for p in &f.parameters {
        if is_tx_context(&p.ty) {
            continue;
        }

        let arg_ident = format_ident!("arg{arg_idx}");
        arg_idx += 1;
        let (ref_mutable, inner) = split_ref(&p.ty);

        let is_object = is_object_type(inner, f, pkg, opts);

        if is_object {
            let obj_ty = types::render_type_ref_in_module(inner, &module.name, pkg, opts);
            let param_ty = if ref_mutable {
                quote! { &mut impl #sm_call::ObjectArg<#obj_ty> }
            } else {
                quote! { &impl #sm_call::ObjectArg<#obj_ty> }
            };
            params.push(quote! { #arg_ident: #param_ty });
            if ref_mutable {
                pushes.push(quote! { spec.push_arg_mut(#arg_ident)?; });
            } else {
                pushes.push(quote! { spec.push_arg(#arg_ident)?; });
            }
        } else {
            let value_ty = types::render_type_ref_in_module(inner, &module.name, pkg, opts);
            params.push(quote! { #arg_ident: #value_ty });
            pushes.push(quote! { spec.push_arg(&#arg_ident)?; });
        }
    }

    (params, pushes)
}

fn has_tx_context(f: &Function) -> bool {
    f.parameters.iter().any(|p| is_tx_context(&p.ty))
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

pub(super) fn type_param_bounds(
    f: &Function,
    pkg: &NormalizedPackage,
    use_aliases: bool,
) -> Vec<TokenStream> {
    let sm = if use_aliases {
        quote! { sm }
    } else {
        quote! { sui_move }
    };

    let required = required_type_param_abilities(f, pkg);

    f.type_parameters
        .iter()
        .enumerate()
        .map(|(idx, _)| {
            let ty = format_ident!("T{idx}");

            let abilities = required.get(&idx).cloned().unwrap_or_default();

            let base = if abilities.contains(&Ability::Key) {
                quote! { #sm::MoveStruct }
            } else {
                quote! { #sm::MoveType }
            };

            let mut bounds: Vec<TokenStream> = vec![base];
            for a in abilities {
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

fn required_type_param_abilities(
    f: &Function,
    pkg: &NormalizedPackage,
) -> BTreeMap<usize, BTreeSet<Ability>> {
    let mut required: BTreeMap<usize, BTreeSet<Ability>> = f
        .type_parameters
        .iter()
        .enumerate()
        .map(|(idx, p)| (idx, p.constraints.iter().cloned().collect()))
        .collect();

    for p in &f.parameters {
        if is_tx_context(&p.ty) {
            continue;
        }

        let (_, inner) = split_ref(&p.ty);
        collect_move_type_bounds(inner, pkg, &mut required);
    }

    required
}

fn collect_move_type_bounds(
    ty: &TypeRef,
    pkg: &NormalizedPackage,
    required: &mut BTreeMap<usize, BTreeSet<Ability>>,
) {
    match ty {
        TypeRef::Vector(inner) => collect_move_type_bounds(inner, pkg, required),
        TypeRef::Ref { inner, .. } => collect_move_type_bounds(inner, pkg, required),
        TypeRef::Datatype {
            type_name,
            type_arguments,
        } => {
            if let Some(dt) = find_local_datatype(pkg, type_name) {
                collect_datatype_impl_bounds(dt, type_arguments, pkg, required);
            }
        }
        _ => {}
    }
}

fn collect_ability_bounds(
    ty: &TypeRef,
    ability: Ability,
    pkg: &NormalizedPackage,
    required: &mut BTreeMap<usize, BTreeSet<Ability>>,
) {
    match ty {
        TypeRef::TypeParameter(idx) => {
            required.entry(*idx as usize).or_default().insert(ability);
        }
        TypeRef::Vector(inner) => collect_ability_bounds(inner, ability, pkg, required),
        TypeRef::Ref { inner, .. } => collect_ability_bounds(inner, ability, pkg, required),
        TypeRef::Datatype {
            type_name,
            type_arguments,
        } => {
            if let Some(dt) = find_local_datatype(pkg, type_name) {
                collect_datatype_impl_bounds(dt, type_arguments, pkg, required);
            }
        }
        _ => {}
    }
}

fn collect_datatype_impl_bounds(
    dt: &Datatype,
    type_arguments: &[TypeRef],
    pkg: &NormalizedPackage,
    required: &mut BTreeMap<usize, BTreeSet<Ability>>,
) {
    for (idx, param) in dt.type_parameters.iter().enumerate() {
        let Some(arg) = type_arguments.get(idx) else {
            continue;
        };

        collect_move_type_bounds(arg, pkg, required);
        for ability in &param.constraints {
            collect_ability_bounds(arg, ability.clone(), pkg, required);
        }
    }

    match &dt.kind {
        DatatypeKind::Struct { fields } => {
            collect_field_ability_bounds(fields, &dt.abilities, type_arguments, pkg, required);
        }
        DatatypeKind::Enum { variants } => {
            for variant in variants {
                collect_field_ability_bounds(
                    &variant.fields,
                    &dt.abilities,
                    type_arguments,
                    pkg,
                    required,
                );
            }
        }
    }
}

fn collect_field_ability_bounds(
    fields: &[Field],
    abilities: &[Ability],
    type_arguments: &[TypeRef],
    pkg: &NormalizedPackage,
    required: &mut BTreeMap<usize, BTreeSet<Ability>>,
) {
    for field in fields {
        let ty = substitute_type_params(&field.ty, type_arguments);
        for ability in abilities {
            match ability {
                Ability::Copy | Ability::Drop | Ability::Store => {
                    collect_ability_bounds(&ty, ability.clone(), pkg, required);
                }
                Ability::Key => {}
            }
        }
    }
}

fn substitute_type_params(ty: &TypeRef, type_arguments: &[TypeRef]) -> TypeRef {
    match ty {
        TypeRef::Vector(inner) => {
            TypeRef::Vector(Box::new(substitute_type_params(inner, type_arguments)))
        }
        TypeRef::Ref { mutable, inner } => TypeRef::Ref {
            mutable: *mutable,
            inner: Box::new(substitute_type_params(inner, type_arguments)),
        },
        TypeRef::Datatype {
            type_name,
            type_arguments: args,
        } => TypeRef::Datatype {
            type_name: type_name.clone(),
            type_arguments: args
                .iter()
                .map(|arg| substitute_type_params(arg, type_arguments))
                .collect(),
        },
        TypeRef::TypeParameter(idx) => type_arguments
            .get(*idx as usize)
            .cloned()
            .unwrap_or(TypeRef::TypeParameter(*idx)),
        other => other.clone(),
    }
}

fn find_local_datatype<'a>(
    pkg: &'a NormalizedPackage,
    type_name: &TypeName,
) -> Option<&'a Datatype> {
    if type_name.address != pkg.storage_id
        && match &pkg.original_id {
            Some(original_id) => type_name.address != *original_id,
            None => true,
        }
    {
        return None;
    }

    pkg.modules
        .get(&type_name.module)?
        .datatypes
        .iter()
        .find(|dt| dt.type_name.name == type_name.name)
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
