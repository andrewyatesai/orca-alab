// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0
// Author: The aterm Authors

//! aterm-spec-macros: Proc macros for TLA+ specification refinement attributes.

#![deny(unsafe_op_in_unsafe_fn)]
#![deny(clippy::all)]

use proc_macro::TokenStream;
use quote::quote;
use syn::{DeriveInput, parse_macro_input};

#[proc_macro_derive(SpecState, attributes(spec_machine))]
pub fn derive_spec_state(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;

    let mut machine_name = None;
    let mut tla_file = None;

    for attr in &input.attrs {
        if attr.path().is_ident("spec_machine") {
            let _ = attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("name") {
                    let value = meta.value()?;
                    let lit: syn::LitStr = value.parse()?;
                    machine_name = Some(lit.value());
                } else if meta.path.is_ident("tla_file") {
                    let value = meta.value()?;
                    let lit: syn::LitStr = value.parse()?;
                    tla_file = Some(lit.value());
                }
                Ok(())
            });
        }
    }

    let machine_name_str =
        machine_name.unwrap_or_else(|| name.to_string().trim_end_matches("Model").to_lowercase());
    let tla_file_str = tla_file.unwrap_or_default();

    let expanded = quote! {
        impl #name {
            pub const SPEC_MACHINE_NAME: &'static str = #machine_name_str;
            pub const SPEC_TLA_FILE: &'static str = #tla_file_str;
        }
    };

    TokenStream::from(expanded)
}

#[proc_macro_derive(SpecAction)]
pub fn derive_spec_action(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;

    let variant_names: Vec<String> = match &input.data {
        syn::Data::Enum(data) => data.variants.iter().map(|v| v.ident.to_string()).collect(),
        _ => {
            return syn::Error::new_spanned(name, "SpecAction can only be derived for enums")
                .to_compile_error()
                .into();
        }
    };

    let count = variant_names.len();

    let expanded = quote! {
        impl #name {
            pub const SPEC_ACTIONS: [&'static str; #count] = [
                #(#variant_names),*
            ];
        }
    };

    TokenStream::from(expanded)
}

#[proc_macro_attribute]
pub fn refines(attr: TokenStream, item: TokenStream) -> TokenStream {
    let meta = syn::parse_macro_input!(attr as RefinesMeta);
    let _ = meta;
    item
}

#[proc_macro_attribute]
pub fn spec_invariant(attr: TokenStream, item: TokenStream) -> TokenStream {
    let meta = syn::parse_macro_input!(attr as InvariantMeta);
    let _ = meta;
    item
}

#[proc_macro_attribute]
pub fn spec_unmodeled(attr: TokenStream, item: TokenStream) -> TokenStream {
    let meta = syn::parse_macro_input!(attr as UnmodeledMeta);
    let _ = meta;
    item
}

struct RefinesMeta {
    _machine: String,
    _action: String,
}

impl syn::parse::Parse for RefinesMeta {
    fn parse(input: syn::parse::ParseStream<'_>) -> syn::Result<Self> {
        let mut machine = None;
        let mut action = None;
        while !input.is_empty() {
            let ident: syn::Ident = input.parse()?;
            let _: syn::Token![=] = input.parse()?;
            let lit: syn::LitStr = input.parse()?;
            match ident.to_string().as_str() {
                "machine" => machine = Some(lit.value()),
                "action" => action = Some(lit.value()),
                other => {
                    return Err(syn::Error::new(
                        ident.span(),
                        format!("unknown key: {other}"),
                    ));
                }
            }
            if !input.is_empty() {
                let _: syn::Token![,] = input.parse()?;
            }
        }
        Ok(RefinesMeta {
            _machine: machine
                .ok_or_else(|| syn::Error::new(input.span(), "missing `machine` key"))?,
            _action: action.ok_or_else(|| syn::Error::new(input.span(), "missing `action` key"))?,
        })
    }
}

struct InvariantMeta {
    _id: String,
    _tla: Option<String>,
}

impl syn::parse::Parse for InvariantMeta {
    fn parse(input: syn::parse::ParseStream<'_>) -> syn::Result<Self> {
        let mut id = None;
        let mut tla = None;
        while !input.is_empty() {
            let ident: syn::Ident = input.parse()?;
            let _: syn::Token![=] = input.parse()?;
            let lit: syn::LitStr = input.parse()?;
            match ident.to_string().as_str() {
                "id" => id = Some(lit.value()),
                "tla" => tla = Some(lit.value()),
                other => {
                    return Err(syn::Error::new(
                        ident.span(),
                        format!("unknown key: {other}"),
                    ));
                }
            }
            if !input.is_empty() {
                let _: syn::Token![,] = input.parse()?;
            }
        }
        Ok(InvariantMeta {
            _id: id.ok_or_else(|| syn::Error::new(input.span(), "missing `id` key"))?,
            _tla: tla,
        })
    }
}

struct UnmodeledMeta {
    _reason: String,
}

impl syn::parse::Parse for UnmodeledMeta {
    fn parse(input: syn::parse::ParseStream<'_>) -> syn::Result<Self> {
        let mut reason = None;
        while !input.is_empty() {
            let ident: syn::Ident = input.parse()?;
            let _: syn::Token![=] = input.parse()?;
            let lit: syn::LitStr = input.parse()?;
            match ident.to_string().as_str() {
                "reason" => reason = Some(lit.value()),
                other => {
                    return Err(syn::Error::new(
                        ident.span(),
                        format!("unknown key: {other}"),
                    ));
                }
            }
            if !input.is_empty() {
                let _: syn::Token![,] = input.parse()?;
            }
        }
        Ok(UnmodeledMeta {
            _reason: reason.ok_or_else(|| syn::Error::new(input.span(), "missing `reason` key"))?,
        })
    }
}

// ---------------------------------------------------------------------------
// ty_model! — the "light annotation" surface for aterm-spec's derived TLA+.
//
// Write a bounded state machine as near-plain Rust; expands to an
// `::aterm_spec::derive::Model` literal (the single source from which both the
// `ty`-checkable TLA+ spec and the executable interpreter are generated). E.g.:
//
//   ty_model! {
//       Ring {
//           const MaxSeq = 6;
//           const Cap = 3;
//           var seq = 0;
//           var lo = 1;
//           action Push when (seq <= MaxSeq - 1) {
//               seq = seq + 1;
//               lo = if (seq + 1) - lo + 1 > Cap { lo + 1 } else { lo };
//           }
//           invariant LenBounded: seq - lo + 1 <= Cap;
//       }
//   }
//
// Identifiers declared `const` become TLA+ CONSTANTS; everything else resolves to
// a state variable. Supported operators: + - > <= and if/else (mapped to the
// `Expr` builders in `aterm_spec::derive`). The macro emits absolute
// `::aterm_spec::derive::*` paths, so it works in any crate depending on
// aterm-spec (and in aterm-spec's own integration tests).
// ---------------------------------------------------------------------------

/// See module-level note above. Expands to an `::aterm_spec::derive::Model`.
#[proc_macro]
pub fn ty_model(input: TokenStream) -> TokenStream {
    let def = parse_macro_input!(input as ModelDef);
    match def.expand() {
        Ok(ts) => ts.into(),
        Err(e) => e.to_compile_error().into(),
    }
}

struct ActionDef {
    name: syn::Ident,
    guard: Option<syn::Expr>,
    updates: Vec<(syn::Ident, syn::Expr)>,
}

struct ModelDef {
    name: syn::Ident,
    consts: Vec<(syn::Ident, i64)>,
    vars: Vec<(syn::Ident, i64)>,
    actions: Vec<ActionDef>,
    invariants: Vec<(syn::Ident, syn::Expr)>,
}

impl syn::parse::Parse for ModelDef {
    fn parse(input: syn::parse::ParseStream<'_>) -> syn::Result<Self> {
        let name: syn::Ident = input.parse()?;
        let body;
        syn::braced!(body in input);
        let mut consts = Vec::new();
        let mut vars = Vec::new();
        let mut actions = Vec::new();
        let mut invariants = Vec::new();
        while !body.is_empty() {
            if body.peek(syn::Token![const]) {
                body.parse::<syn::Token![const]>()?;
                let id: syn::Ident = body.parse()?;
                body.parse::<syn::Token![=]>()?;
                let lit: syn::LitInt = body.parse()?;
                body.parse::<syn::Token![;]>()?;
                consts.push((id, lit.base10_parse::<i64>()?));
                continue;
            }
            let kw: syn::Ident = body.parse()?;
            match kw.to_string().as_str() {
                "var" => {
                    let id: syn::Ident = body.parse()?;
                    body.parse::<syn::Token![=]>()?;
                    let lit: syn::LitInt = body.parse()?;
                    body.parse::<syn::Token![;]>()?;
                    vars.push((id, lit.base10_parse::<i64>()?));
                }
                "action" => {
                    let aname: syn::Ident = body.parse()?;
                    let guard = if body.peek(syn::Ident) {
                        let w: syn::Ident = body.parse()?;
                        if w != "when" {
                            return Err(syn::Error::new(w.span(), "expected `when` or `{`"));
                        }
                        let g;
                        syn::parenthesized!(g in body);
                        Some(g.parse::<syn::Expr>()?)
                    } else {
                        None
                    };
                    let ab;
                    syn::braced!(ab in body);
                    let mut updates = Vec::new();
                    while !ab.is_empty() {
                        let lhs: syn::Ident = ab.parse()?;
                        ab.parse::<syn::Token![=]>()?;
                        let rhs: syn::Expr = ab.parse()?;
                        ab.parse::<syn::Token![;]>()?;
                        updates.push((lhs, rhs));
                    }
                    actions.push(ActionDef { name: aname, guard, updates });
                }
                "invariant" => {
                    let iname: syn::Ident = body.parse()?;
                    body.parse::<syn::Token![:]>()?;
                    let e: syn::Expr = body.parse()?;
                    body.parse::<syn::Token![;]>()?;
                    invariants.push((iname, e));
                }
                other => {
                    return Err(syn::Error::new(
                        kw.span(),
                        format!("expected const/var/action/invariant, found `{other}`"),
                    ));
                }
            }
        }
        Ok(ModelDef { name, consts, vars, actions, invariants })
    }
}

impl ModelDef {
    fn expand(&self) -> syn::Result<proc_macro2::TokenStream> {
        let const_names: std::collections::HashSet<String> =
            self.consts.iter().map(|(id, _)| id.to_string()).collect();
        let name_str = self.name.to_string();

        let consts_toks = self.consts.iter().map(|(id, v)| {
            let s = id.to_string();
            quote! { (#s, #v) }
        });
        let vars_toks = self.vars.iter().map(|(id, v)| {
            let s = id.to_string();
            quote! { ::aterm_spec::derive::StateVar { name: #s, init: #v } }
        });

        let mut actions_toks = Vec::new();
        for a in &self.actions {
            let an = a.name.to_string();
            let guard = match &a.guard {
                Some(g) => {
                    let t = tr_expr(g, &const_names)?;
                    quote! { Some(#t) }
                }
                None => quote! { None },
            };
            let mut ups = Vec::new();
            for (lhs, rhs) in &a.updates {
                let s = lhs.to_string();
                let t = tr_expr(rhs, &const_names)?;
                ups.push(quote! { ::aterm_spec::derive::Update { var: #s, expr: #t } });
            }
            actions_toks.push(quote! {
                ::aterm_spec::derive::Action { name: #an, guard: #guard, updates: vec![ #(#ups),* ] }
            });
        }

        let mut invs_toks = Vec::new();
        for (id, e) in &self.invariants {
            let s = id.to_string();
            let t = tr_expr(e, &const_names)?;
            invs_toks.push(quote! { ::aterm_spec::derive::Invariant { name: #s, expr: #t } });
        }

        Ok(quote! {
            ::aterm_spec::derive::Model {
                name: #name_str,
                consts: vec![ #(#consts_toks),* ],
                vars: vec![ #(#vars_toks),* ],
                fn_vars: vec![],
                actions: vec![ #(#actions_toks),* ],
                invariants: vec![ #(#invs_toks),* ],
            }
        })
    }
}

/// The single tail expression of a one-expression block (`{ expr }`).
fn block_tail(block: &syn::Block) -> syn::Result<&syn::Expr> {
    if let [syn::Stmt::Expr(e, None)] = block.stmts.as_slice() {
        Ok(e)
    } else {
        Err(syn::Error::new_spanned(block, "if/else branch must be a single expression"))
    }
}

/// Translate the else branch (a `{ block }` or a nested `if`) to an `Expr`.
fn tr_else(e: &syn::Expr, consts: &std::collections::HashSet<String>) -> syn::Result<proc_macro2::TokenStream> {
    match e {
        syn::Expr::Block(b) => tr_expr(block_tail(&b.block)?, consts),
        _ => tr_expr(e, consts),
    }
}

/// Translate a restricted `syn::Expr` to `aterm_spec::derive::Expr` builder calls.
fn tr_expr(e: &syn::Expr, consts: &std::collections::HashSet<String>) -> syn::Result<proc_macro2::TokenStream> {
    match e {
        syn::Expr::Lit(syn::ExprLit { lit: syn::Lit::Int(i), .. }) => {
            let v: i64 = i.base10_parse()?;
            Ok(quote! { ::aterm_spec::derive::int(#v) })
        }
        syn::Expr::Path(p) => {
            if let Some(id) = p.path.get_ident() {
                let id = id.to_string();
                if consts.contains(&id) {
                    Ok(quote! { ::aterm_spec::derive::cst(#id) })
                } else {
                    Ok(quote! { ::aterm_spec::derive::var(#id) })
                }
            } else {
                Err(syn::Error::new_spanned(e, "expected a simple identifier"))
            }
        }
        syn::Expr::Paren(p) => tr_expr(&p.expr, consts),
        syn::Expr::Binary(b) => {
            let l = tr_expr(&b.left, consts)?;
            let r = tr_expr(&b.right, consts)?;
            let f = match b.op {
                syn::BinOp::Add(_) => quote! { ::aterm_spec::derive::add },
                syn::BinOp::Sub(_) => quote! { ::aterm_spec::derive::sub },
                syn::BinOp::Gt(_) => quote! { ::aterm_spec::derive::gt },
                syn::BinOp::Le(_) => quote! { ::aterm_spec::derive::le },
                _ => return Err(syn::Error::new_spanned(e, "unsupported operator (use + - > <=)")),
            };
            Ok(quote! { #f(#l, #r) })
        }
        syn::Expr::If(ifx) => {
            let Some((_, else_expr)) = &ifx.else_branch else {
                return Err(syn::Error::new_spanned(e, "`if` must have an `else` branch"));
            };
            let c = tr_expr(&ifx.cond, consts)?;
            let t = tr_expr(block_tail(&ifx.then_branch)?, consts)?;
            let f = tr_else(else_expr, consts)?;
            Ok(quote! { ::aterm_spec::derive::if_(#c, #t, #f) })
        }
        other => Err(syn::Error::new_spanned(other, "unsupported expression in ty_model!")),
    }
}
