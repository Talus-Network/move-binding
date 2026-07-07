//! Small helpers for rendering and formatting.

use proc_macro2::TokenStream;
use quote::quote;

use crate::ir::{NormalizedModule, NormalizedPackage};

use super::{calls, idents, tx_ext, types, RenderOptions};

pub(crate) fn render_package_tokens(pkg: &NormalizedPackage, opts: &RenderOptions) -> TokenStream {
    let mut modules = Vec::new();
    for module in pkg.modules.values() {
        modules.push(render_module(module, pkg, opts));
    }

    let root = render_package_root_tokens(pkg, opts, false);

    quote! {
        #root
        #(#modules)*
    }
}

pub(crate) fn render_split_mod_rs_tokens(
    pkg: &NormalizedPackage,
    opts: &RenderOptions,
) -> TokenStream {
    render_package_root_tokens(pkg, opts, true)
}

pub(crate) fn render_package_root_tokens(
    pkg: &NormalizedPackage,
    opts: &RenderOptions,
    emit_module_decls: bool,
) -> TokenStream {
    let root_aliases = if opts.emit_tx_ext && (emit_module_decls || !opts.flatten) {
        aliases(opts)
    } else {
        quote! {}
    };

    let package_const = package_const_tokens(pkg, opts);
    let package_scope = package_scope_tokens(opts);

    let module_decls = if emit_module_decls {
        let module_decls = pkg.modules.values().map(|module| {
            let module_ident = idents::ident(&module.name);
            quote! { pub mod #module_ident; }
        });
        quote! { #(#module_decls)* }
    } else {
        quote! {}
    };

    let reexports = if !opts.emit_types || !opts.emit_reexports {
        quote! {}
    } else {
        let mut out = Vec::new();
        for module in pkg.modules.values() {
            let module_ident = idents::ident(&module.name);
            for dt in &module.datatypes {
                let ty_ident = idents::ident(&dt.name);
                out.push(quote! { pub use #module_ident::#ty_ident; });
            }
        }
        quote! { #(#out)* }
    };

    let tx_ext = if opts.emit_tx_ext {
        tx_ext::render_tx_ext(pkg, opts)
    } else {
        quote! {}
    };

    quote! {
        #root_aliases
        #package_const
        #package_scope
        #module_decls
        #tx_ext
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
        use super::{call_package, type_package};
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
                use super::{call_package, type_package};
                #(#items)*
            }
        }
    }
}

fn package_const_tokens(pkg: &NormalizedPackage, opts: &RenderOptions) -> TokenStream {
    let call_addr = pkg.storage_id.as_str();
    let type_addr = pkg
        .original_id
        .as_deref()
        .unwrap_or(pkg.storage_id.as_str());
    let _ = opts;
    let address_ty = quote! { sui_move::prelude::Address };
    quote! {
        /// Package address used as the target for generated Move calls.
        pub const CALL_PACKAGE: #address_ty = #address_ty::from_static(#call_addr);

        /// Package address used for generated Move type identity.
        pub const TYPE_PACKAGE: #address_ty = #address_ty::from_static(#type_addr);
    }
}

fn package_scope_tokens(opts: &RenderOptions) -> TokenStream {
    let _ = opts;
    let address_ty = quote! { sui_move::prelude::Address };
    quote! {
        std::thread_local! {
            static CALL_PACKAGE_OVERRIDE: std::cell::Cell<Option<#address_ty>> =
                std::cell::Cell::new(None);
            static TYPE_PACKAGE_OVERRIDE: std::cell::Cell<Option<#address_ty>> =
                std::cell::Cell::new(None);
        }

        /// Current call package address for this generated binding.
        ///
        /// Returns the scoped override set by [`with_call_package`] or [`with_packages`], or
        /// [`CALL_PACKAGE`] when no override is active.
        pub fn call_package() -> #address_ty {
            CALL_PACKAGE_OVERRIDE.with(|slot| slot.get().unwrap_or(CALL_PACKAGE))
        }

        /// Current type package address for this generated binding.
        ///
        /// Returns the scoped override set by [`with_type_package`] or [`with_packages`], or
        /// [`TYPE_PACKAGE`] when no override is active.
        pub fn type_package() -> #address_ty {
            TYPE_PACKAGE_OVERRIDE.with(|slot| slot.get().unwrap_or(TYPE_PACKAGE))
        }

        /// Run a closure with this generated binding scoped to `package` for Move calls.
        ///
        /// The previous call package override is restored when the closure returns or unwinds.
        pub fn with_call_package<R>(package: #address_ty, f: impl FnOnce() -> R) -> R {
            struct Reset(Option<#address_ty>);

            impl Drop for Reset {
                fn drop(&mut self) {
                    CALL_PACKAGE_OVERRIDE.with(|slot| slot.set(self.0));
                }
            }

            let previous = CALL_PACKAGE_OVERRIDE.with(|slot| {
                let previous = slot.get();
                slot.set(Some(package));
                previous
            });
            let _reset = Reset(previous);
            f()
        }

        /// Run a closure with this generated binding scoped to `package` for Move type identity.
        ///
        /// The previous type package override is restored when the closure returns or unwinds.
        pub fn with_type_package<R>(package: #address_ty, f: impl FnOnce() -> R) -> R {
            struct Reset(Option<#address_ty>);

            impl Drop for Reset {
                fn drop(&mut self) {
                    TYPE_PACKAGE_OVERRIDE.with(|slot| slot.set(self.0));
                }
            }

            let previous = TYPE_PACKAGE_OVERRIDE.with(|slot| {
                let previous = slot.get();
                slot.set(Some(package));
                previous
            });
            let _reset = Reset(previous);
            f()
        }

        /// Run a closure with explicit call and type package scopes.
        ///
        /// Use this for upgraded packages where calls target the current package object but type
        /// tags must retain the original defining package address.
        pub fn with_packages<R>(
            call_package: #address_ty,
            type_package: #address_ty,
            f: impl FnOnce() -> R,
        ) -> R {
            with_call_package(call_package, || with_type_package(type_package, f))
        }
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
