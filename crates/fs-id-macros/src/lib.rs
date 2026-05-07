//! `#[fs_id]` proc macro. Implementation crate; users depend on `fs-id`
//! and import the macro through its re-export.

use proc_macro::TokenStream;
use quote::quote;
use syn::{
    Error, Fields, ItemStruct, LitStr, Result, Token, Type, parse::Parse, parse::ParseStream,
    parse_macro_input,
};

/// Attach to a tuple struct with a single field whose type is one of the
/// allow-listed inner types ([`Nanoid`], [`Uuid`], [`Ulid`], `u64`, `u32`,
/// `i64`, `i32`).
///
/// Emits a branded ID type with serde, ts-rs, utoipa, `Display`,
/// `From<Inner>`, and a `new()` constructor (auto-generating for
/// [`Nanoid`] / [`Uuid`] / [`Ulid`], value-taking for numeric inners).
///
/// The brand string is constructed at macro expansion time from the
/// struct's own ident — it cannot drift.
///
/// Optional argument: `export_to = "..."` sets ts-rs's per-type export
/// path. If omitted, ts-rs's default applies.
#[proc_macro_attribute]
pub fn fs_id(args: TokenStream, input: TokenStream) -> TokenStream {
    let args = parse_macro_input!(args as Args);
    let item = parse_macro_input!(input as ItemStruct);

    expand(args, item)
        .unwrap_or_else(Error::into_compile_error)
        .into()
}

struct Args {
    export_to: Option<LitStr>,
}

impl Parse for Args {
    fn parse(input: ParseStream) -> Result<Self> {
        let mut export_to = None;
        let mut first = true;
        while !input.is_empty() {
            if !first {
                input.parse::<Token![,]>()?;
            }
            first = false;

            let key: syn::Ident = input.parse()?;
            input.parse::<Token![=]>()?;

            match key.to_string().as_str() {
                "export_to" => {
                    let lit: LitStr = input.parse()?;
                    export_to = Some(lit);
                }
                "brand" => {
                    return Err(Error::new(
                        key.span(),
                        "`brand` is intentionally not supported. The brand \
                         string is derived from the struct ident; allowing \
                         overrides would let it drift from the type name.",
                    ));
                }
                other => {
                    return Err(Error::new(
                        key.span(),
                        format!("unknown argument `{other}`. Only `export_to` is supported."),
                    ));
                }
            }
        }
        Ok(Self { export_to })
    }
}

#[derive(Clone, Copy)]
enum Primitive {
    String,
    Number,
}

impl Primitive {
    fn as_str(self) -> &'static str {
        match self {
            Self::String => "string",
            Self::Number => "number",
        }
    }
}

#[derive(Clone, Copy)]
struct InnerKind {
    primitive: Primitive,
    auto: bool,
    copy: bool,
    /// utoipa `value_type` selection.
    schema_value_type: &'static str,
    /// utoipa `format` selection.
    schema_format: &'static str,
}

/// The allow-list. Adding a new inner type:
/// 1. Add an `impl BrandedInner` (and optionally `BrandedInnerAuto`) in
///    `crates/fs-id/src/lib.rs`.
/// 2. Add a match arm here with the right (primitive, auto, copy, schema)
///    quadruple.
fn classify(ident: &syn::Ident) -> Option<InnerKind> {
    Some(match ident.to_string().as_str() {
        "Nanoid" => InnerKind {
            primitive: Primitive::String,
            auto: true,
            copy: false,
            schema_value_type: "String",
            schema_format: "nanoid",
        },
        "Uuid" => InnerKind {
            primitive: Primitive::String,
            auto: true,
            copy: false,
            schema_value_type: "String",
            schema_format: "uuid",
        },
        "Ulid" => InnerKind {
            primitive: Primitive::String,
            auto: true,
            copy: false,
            schema_value_type: "String",
            schema_format: "ulid",
        },
        "u64" => InnerKind {
            primitive: Primitive::Number,
            auto: false,
            copy: true,
            schema_value_type: "u64",
            schema_format: "int64",
        },
        "u32" => InnerKind {
            primitive: Primitive::Number,
            auto: false,
            copy: true,
            schema_value_type: "u32",
            schema_format: "int32",
        },
        "i64" => InnerKind {
            primitive: Primitive::Number,
            auto: false,
            copy: true,
            schema_value_type: "i64",
            schema_format: "int64",
        },
        "i32" => InnerKind {
            primitive: Primitive::Number,
            auto: false,
            copy: true,
            schema_value_type: "i32",
            schema_format: "int32",
        },
        _ => return None,
    })
}

fn last_segment_ident(ty: &Type) -> Option<syn::Ident> {
    if let Type::Path(p) = ty {
        p.path.segments.last().map(|s| s.ident.clone())
    } else {
        None
    }
}

fn expand(args: Args, item: ItemStruct) -> Result<proc_macro2::TokenStream> {
    let ItemStruct {
        attrs,
        vis,
        struct_token: _,
        ident,
        generics,
        fields,
        semi_token: _,
    } = item;

    if !generics.params.is_empty() {
        return Err(Error::new_spanned(
            &generics,
            "`#[fs_id]` does not support generic structs",
        ));
    }

    let field = match &fields {
        Fields::Unnamed(f) if f.unnamed.len() == 1 => f.unnamed.first().unwrap(),
        _ => {
            return Err(Error::new_spanned(
                &fields,
                "`#[fs_id]` requires a tuple struct with exactly one field, \
                 e.g. `pub struct Foo(pub Bar);`",
            ));
        }
    };

    let field_vis = &field.vis;
    let inner_ty = &field.ty;

    let inner_ident = last_segment_ident(inner_ty).ok_or_else(|| {
        Error::new_spanned(
            inner_ty,
            "`#[fs_id]` could not read the inner type's last path segment",
        )
    })?;

    let kind = classify(&inner_ident).ok_or_else(|| {
        Error::new_spanned(
            inner_ty,
            format!(
                "`#[fs_id]` does not recognize inner type `{inner_ident}`. \
                 Implement `BrandedInner` (and optionally `BrandedInnerAuto`) \
                 for it in `fs-id`, then add it to the allow-list in \
                 `fs-id-macros/src/lib.rs`.",
            ),
        )
    })?;

    let brand_str = format!(
        "{} & {{ readonly __brand: \"{}\" }}",
        kind.primitive.as_str(),
        ident
    );
    let brand_lit = LitStr::new(&brand_str, ident.span());

    // Only emit `#[ts(export)]` when `export_to` is provided. Without it,
    // ts-rs would write the file under `TS_RS_EXPORT_DIR` (workspace
    // `packages/`), which leaks test types into the production tree.
    let ts_attr = match &args.export_to {
        Some(path) => quote! { #[ts(export, export_to = #path, type = #brand_lit)] },
        None => quote! { #[ts(type = #brand_lit)] },
    };

    let schema_value_type: syn::Type =
        syn::parse_str(kind.schema_value_type).expect("hard-coded schema_value_type must parse");
    let schema_format = LitStr::new(kind.schema_format, ident.span());

    let copy_derive = if kind.copy {
        quote! { , ::core::marker::Copy }
    } else {
        quote! {}
    };

    let new_impl = if kind.auto {
        quote! {
            impl #ident {
                #[allow(clippy::new_without_default)]
                pub fn new() -> Self {
                    Self(<#inner_ty as ::fs_id::BrandedInnerAuto>::generate())
                }
            }
        }
    } else {
        quote! {
            impl #ident {
                pub const fn new(value: #inner_ty) -> Self {
                    Self(value)
                }
            }
        }
    };

    Ok(quote! {
        #(#attrs)*
        #[derive(
            ::core::fmt::Debug,
            ::core::clone::Clone
            #copy_derive,
            ::core::cmp::PartialEq,
            ::core::cmp::Eq,
            ::core::hash::Hash,
            ::serde::Serialize,
            ::serde::Deserialize,
            ::ts_rs::TS,
            ::utoipa::ToSchema,
        )]
        #ts_attr
        #[schema(value_type = #schema_value_type, format = #schema_format)]
        #vis struct #ident(#field_vis #inner_ty);

        impl ::core::convert::From<#inner_ty> for #ident {
            fn from(value: #inner_ty) -> Self {
                Self(value)
            }
        }

        #new_impl

        impl ::core::fmt::Display for #ident {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                ::core::write!(f, "{}", self.0)
            }
        }
    })
}
