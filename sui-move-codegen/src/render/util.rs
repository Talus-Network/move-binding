//! Small helpers for rendering and formatting.

use proc_macro2::TokenStream;
use quote::quote;

use crate::ir::{NormalizedModule, NormalizedPackage};

use super::{calls, idents, types, RenderOptions};

pub(crate) fn render_package_tokens(pkg: &NormalizedPackage, opts: &RenderOptions) -> TokenStream {
    let package_const = package_const_tokens(pkg, opts);

    let mut modules = Vec::new();
    for module in pkg.modules.values() {
        modules.push(render_module(module, pkg, opts));
    }

    let reexports = if opts.flatten || !opts.emit_types {
        quote! {}
    } else {
        let mut reexp = Vec::new();
        for module in pkg.modules.values() {
            let module_ident = idents::ident(&module.name);
            for dt in &module.datatypes {
                let ty_ident = idents::ident(&dt.name);
                reexp.push(quote! { pub use #module_ident::#ty_ident; });
            }
        }
        quote! { #(#reexp)* }
    };

    quote! {
        #package_const
        #(#modules)*
        #reexports
    }
}

pub(crate) fn render_module_file(
    module: &NormalizedModule,
    pkg: &NormalizedPackage,
    opts: &RenderOptions,
) -> TokenStream {
    let aliases = aliases(opts);

    let mut items = Vec::new();
    if opts.emit_types {
        items.extend(
            module
                .datatypes
                .iter()
                .map(|dt| types::render_datatype(dt, pkg, opts)),
        );
    }
    if opts.emit_calls {
        items.extend(calls::render_functions(module, pkg, opts));
    }

    quote! {
        #aliases
        use super::PACKAGE;
        #(#items)*
    }
}

pub(crate) fn render_module(
    module: &NormalizedModule,
    pkg: &NormalizedPackage,
    opts: &RenderOptions,
) -> TokenStream {
    let aliases = aliases(opts);

    let mut items = Vec::new();
    if opts.emit_types {
        items.extend(
            module
                .datatypes
                .iter()
                .map(|dt| types::render_datatype(dt, pkg, opts)),
        );
    }
    if opts.emit_calls {
        items.extend(calls::render_functions(module, pkg, opts));
    }

    if opts.flatten {
        quote! { #aliases #(#items)* }
    } else {
        let module_ident = idents::ident(&module.name);
        quote! {
            pub mod #module_ident {
                #aliases
                use super::PACKAGE;
                #(#items)*
            }
        }
    }
}

fn package_const_tokens(pkg: &NormalizedPackage, opts: &RenderOptions) -> TokenStream {
    let addr = pkg.storage_id.as_str();
    let _ = opts;
    let address_ty = quote! { sui_move::prelude::Address };
    quote! {
        /// Package address (the on-chain package object id).
        pub const PACKAGE: #address_ty = #address_ty::from_static(#addr);
    }
}

fn aliases(opts: &RenderOptions) -> TokenStream {
    if !opts.use_aliases {
        return quote! {};
    }
    quote! {
        #[allow(unused_imports)]
        use sui_move as sm;
        #[allow(unused_imports)]
        use sui_move_call as sm_call;
    }
}

pub(crate) fn prettify(tokens: TokenStream) -> String {
    let formatted = match syn::parse2::<syn::File>(tokens.clone()) {
        Ok(file) => prettyplease::unparse(&file),
        Err(_) => tokens.to_string(),
    };
    insert_item_spacing(&formatted)
}

/// Insert an empty line between items to improve readability of generated code.
pub(crate) fn insert_item_spacing(code: &str) -> String {
    let mut out = String::new();
    let mut lines = code.lines().peekable();

    while let Some(line) = lines.next() {
        let trimmed = line.trim();
        out.push_str(line);
        out.push('\n');

        let mut next_nonempty = None;
        let lookahead = lines.clone();
        for peek in lookahead {
            if !peek.trim().is_empty() {
                next_nonempty = Some(peek.trim().to_string());
                break;
            }
        }

        let starts_new_item = next_nonempty
            .as_deref()
            .map(|next| {
                next.starts_with("#[")
                    || next.starts_with("impl ")
                    || next.starts_with("pub ")
                    || next.starts_with("fn ")
            })
            .unwrap_or(false);

        let is_use_line = trimmed.starts_with("use ") || trimmed.starts_with("pub use ");
        let is_allow_attr = trimmed.starts_with("#[allow");
        let is_mod_line = trimmed.starts_with("pub mod ");

        if trimmed.ends_with('}') && starts_new_item {
            out.push('\n');
        }

        if (is_use_line || is_allow_attr)
            && next_nonempty
                .as_deref()
                .map(|next| {
                    !(next.starts_with("use ")
                        || next.starts_with("pub use ")
                        || next.starts_with("#[allow"))
                })
                .unwrap_or(false)
        {
            out.push('\n');
        }

        if is_mod_line
            && next_nonempty
                .as_deref()
                .map(|next| !next.starts_with("pub mod "))
                .unwrap_or(false)
        {
            out.push('\n');
        }
    }

    out
}
