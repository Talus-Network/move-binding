//! Identifier helpers for rendering Rust code.

use syn::Ident;

/// Turn a Move identifier into a valid Rust identifier.
///
/// Move identifiers are already close to Rust identifiers; this mainly exists to:
/// - handle Rust keywords by emitting raw identifiers (`r#type`)
/// - avoid panic/UB by ensuring we always construct an identifier
pub(crate) fn ident(name: &str) -> Ident {
    if is_keyword(name) {
        Ident::new_raw(name, proc_macro2::Span::call_site())
    } else {
        Ident::new(name, proc_macro2::Span::call_site())
    }
}

pub(crate) fn is_keyword(s: &str) -> bool {
    matches!(
        s,
        "as" | "break"
            | "const"
            | "continue"
            | "crate"
            | "else"
            | "enum"
            | "extern"
            | "false"
            | "fn"
            | "for"
            | "if"
            | "impl"
            | "in"
            | "let"
            | "loop"
            | "match"
            | "mod"
            | "move"
            | "mut"
            | "pub"
            | "ref"
            | "return"
            | "self"
            | "Self"
            | "static"
            | "struct"
            | "super"
            | "trait"
            | "true"
            | "type"
            | "unsafe"
            | "use"
            | "where"
            | "while"
            | "async"
            | "await"
            | "dyn"
            | "abstract"
            | "become"
            | "box"
            | "do"
            | "final"
            | "macro"
            | "override"
            | "priv"
            | "try"
            | "typeof"
            | "unsized"
            | "virtual"
            | "yield"
    )
}
