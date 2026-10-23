//! Paths inside macro token streams. Literals are skipped, so a format string
//! is not a path. A body that does not parse is still walked.

use proc_macro2::{Delimiter, TokenStream, TokenTree};

pub fn ident_paths(stream: &TokenStream) -> Vec<Vec<String>> {
    let mut out = Vec::new();
    collect(stream, &mut out);
    out
}

fn collect(stream: &TokenStream, out: &mut Vec<Vec<String>>) {
    let mut path: Vec<String> = Vec::new();
    let mut colons = 0u8;
    for tree in stream.clone() {
        match tree {
            TokenTree::Ident(ident) => {
                if colons == 1 || (colons == 0 && !path.is_empty()) {
                    push_path(&mut path, out);
                    colons = 0;
                }
                if colons >= 2 {
                    colons = 0;
                }
                path.push(ident.to_string());
            }
            TokenTree::Punct(punct) if punct.as_char() == ':' => {
                colons = colons.saturating_add(1);
                if colons > 2 {
                    push_path(&mut path, out);
                    colons = 0;
                }
            }
            TokenTree::Group(group) => {
                push_path(&mut path, out);
                colons = 0;
                collect(&group.stream(), out);
            }
            _ => {
                push_path(&mut path, out);
                colons = 0;
            }
        }
    }
    push_path(&mut path, out);
}

fn push_path(path: &mut Vec<String>, out: &mut Vec<Vec<String>>) {
    if !path.is_empty() {
        out.push(std::mem::take(path));
    }
}

pub fn macro_expr(tokens: &TokenStream) -> Option<syn::Expr> {
    syn::parse2::<syn::Expr>(tokens.clone())
        .ok()
        .or_else(|| {
            syn::parse2::<syn::Expr>(delimited(Delimiter::Parenthesis, tokens.clone())).ok()
        })
        .or_else(|| syn::parse2::<syn::Expr>(delimited(Delimiter::Brace, tokens.clone())).ok())
}

fn delimited(delimiter: Delimiter, inner: TokenStream) -> TokenStream {
    TokenStream::from(TokenTree::Group(proc_macro2::Group::new(delimiter, inner)))
}
