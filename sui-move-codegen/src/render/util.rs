//! Small helpers for rendering and formatting.

use proc_macro2::TokenStream;
use quote::quote;

use std::collections::BTreeSet;

use crate::ir::{NormalizedModule, NormalizedPackage};

use super::{calls, idents, tx_ext, types, ExternalResolver, RenderOptions};

pub(crate) fn render_package_tokens(
    pkg: &NormalizedPackage,
    opts: &RenderOptions,
    resolver: Option<&ExternalResolver>,
) -> TokenStream {
    let root_aliases = if opts.emit_tx_ext && !opts.flatten {
        aliases(opts)
    } else {
        quote! {}
    };

    let package_const = package_const_tokens(pkg, opts);

    let mut modules = Vec::new();
    for module in pkg.modules.values() {
        modules.push(render_module(module, pkg, opts, resolver));
    }

    let tx_ext = if opts.emit_tx_ext {
        tx_ext::render_tx_ext(pkg, opts, resolver)
    } else {
        quote! {}
    };

    let reexports = if opts.flatten || !opts.emit_types {
        quote! {}
    } else {
        let mut seen: BTreeSet<String> = BTreeSet::new();
        let mut reexp = Vec::new();
        for module in pkg.modules.values() {
            let module_ident = idents::ident(&module.name);
            for dt in &module.datatypes {
                let ty_ident = idents::ident(&dt.name);
                if !seen.insert(ty_ident.to_string()) {
                    continue;
                }
                reexp.push(quote! { pub use #module_ident::#ty_ident; });
            }
        }
        quote! { #(#reexp)* }
    };

    quote! {
        #root_aliases
        #package_const
        #(#modules)*
        #tx_ext
        #reexports
    }
}

pub(crate) fn render_split_mod_rs_tokens(
    pkg: &NormalizedPackage,
    opts: &RenderOptions,
    resolver: Option<&ExternalResolver>,
) -> TokenStream {
    let root_aliases = if opts.emit_tx_ext {
        aliases(opts)
    } else {
        quote! {}
    };

    let package_const = package_const_tokens(pkg, opts);

    let module_decls = pkg.modules.values().map(|module| {
        let module_ident = idents::ident(&module.name);
        quote! { pub mod #module_ident; }
    });

    let reexports = if !opts.emit_types {
        quote! {}
    } else {
        let mut seen: BTreeSet<String> = BTreeSet::new();
        let mut out = Vec::new();
        for module in pkg.modules.values() {
            let module_ident = idents::ident(&module.name);
            for dt in &module.datatypes {
                let ty_ident = idents::ident(&dt.name);
                if !seen.insert(ty_ident.to_string()) {
                    continue;
                }
                out.push(quote! { pub use #module_ident::#ty_ident; });
            }
        }
        quote! { #(#out)* }
    };

    let tx_ext = if opts.emit_tx_ext {
        tx_ext::render_tx_ext(pkg, opts, resolver)
    } else {
        quote! {}
    };

    quote! {
        #root_aliases
        #package_const
        #(#module_decls)*
        #tx_ext
        #reexports
    }
}

pub(crate) fn render_module_file(
    module: &NormalizedModule,
    pkg: &NormalizedPackage,
    opts: &RenderOptions,
    resolver: Option<&ExternalResolver>,
) -> TokenStream {
    let aliases = aliases(opts);

    let mut items = Vec::new();
    if opts.emit_types {
        items.extend(
            module
                .datatypes
                .iter()
                .map(|dt| types::render_datatype(dt, pkg, opts, resolver)),
        );
    }
    if opts.emit_calls {
        items.extend(calls::render_functions(module, pkg, opts, resolver));
    }

    quote! {
        #aliases
        #(#items)*
    }
}

pub(crate) fn render_module(
    module: &NormalizedModule,
    pkg: &NormalizedPackage,
    opts: &RenderOptions,
    resolver: Option<&ExternalResolver>,
) -> TokenStream {
    let aliases = aliases(opts);

    let mut items = Vec::new();
    if opts.emit_types {
        items.extend(
            module
                .datatypes
                .iter()
                .map(|dt| types::render_datatype(dt, pkg, opts, resolver)),
        );
    }
    if opts.emit_calls {
        items.extend(calls::render_functions(module, pkg, opts, resolver));
    }

    if opts.flatten {
        quote! { #aliases #(#items)* }
    } else {
        let module_ident = idents::ident(&module.name);
        quote! {
            pub mod #module_ident {
                #aliases
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

        /// Internal address helpers for this bindings module.
        ///
        /// This is a small escape hatch for two common deployment patterns:
        /// - upgrade: new package object id (`storage_id`), but types keep their defining ids →
        ///   override only [`__sui_move_bindings::call_package`].
        /// - republish: same code published as a brand-new package (new `original_id`) →
        ///   override both calls and local type addresses via [`__sui_move_bindings::republish_to`].
        #[doc(hidden)]
        pub mod __sui_move_bindings {
            use super::PACKAGE;

            type Address = #address_ty;

            static CALL_PACKAGE_OVERRIDE: ::std::sync::OnceLock<Address> =
                ::std::sync::OnceLock::new();
            static LOCAL_TYPE_ADDRESS_OVERRIDE: ::std::sync::OnceLock<Address> =
                ::std::sync::OnceLock::new();

            /// Package address used for generated call stubs.
            ///
            /// Defaults to [`PACKAGE`]. Can be overridden once via [`set_call_package`].
            #[must_use]
            pub fn call_package() -> Address {
                CALL_PACKAGE_OVERRIDE.get().copied().unwrap_or(PACKAGE)
            }

            /// Override the package address used for generated call stubs.
            ///
            /// Use this for upgrades (same `original_id`, new `storage_id`).
            pub fn set_call_package(addr: Address) -> Result<(), Address> {
                CALL_PACKAGE_OVERRIDE.set(addr)
            }

            /// Override the address used for local type tags.
            ///
            /// Use this for republishing the same code as a new package id.
            pub fn set_local_type_address_override(addr: Address) -> Result<(), Address> {
                LOCAL_TYPE_ADDRESS_OVERRIDE.set(addr)
            }

            /// Convenience for the republish case: retarget both calls and local type tags.
            pub fn republish_to(addr: Address) -> Result<(), Address> {
                set_call_package(addr)?;
                set_local_type_address_override(addr)?;
                Ok(())
            }

            /// Resolve the defining address for a local type.
            ///
            /// When republishing the same code, you can override all local type addresses by
            /// calling [`set_local_type_address_override`]. For upgrades, keep the default per-type
            /// defining id.
            #[must_use]
            pub fn local_type_address(default: Address) -> Address {
                LOCAL_TYPE_ADDRESS_OVERRIDE.get().copied().unwrap_or(default)
            }
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
