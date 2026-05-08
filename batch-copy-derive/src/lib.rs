use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{
    parse_macro_input, punctuated::Punctuated, Data, DeriveInput, Expr, ExprLit, Fields,
    GenericArgument, Ident, Lit, Meta, PathArguments, Token, Type,
};

/// Derives `BatchCopyRow` for a named-field struct.
///
/// The table name defaults to the snake_case of the struct name.
/// Override it with `#[batch_copy(table = "my_table")]`.
///
/// Field types are mapped to PostgreSQL types automatically for common types.
/// For unknown types, annotate the field with `#[pg(TYPE)]`, e.g. `#[pg(TIMESTAMPTZ)]`.
#[proc_macro_derive(BatchCopy, attributes(batch_copy, pg))]
pub fn derive_batch_copy(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match derive_impl(input) {
        Ok(ts) => ts.into(),
        Err(e) => e.to_compile_error().into(),
    }
}

fn derive_impl(input: DeriveInput) -> syn::Result<TokenStream2> {
    let name = &input.ident;

    let table_name = extract_table_name(&input)?;

    let fields = match &input.data {
        Data::Struct(s) => match &s.fields {
            Fields::Named(f) => &f.named,
            _ => {
                return Err(syn::Error::new_spanned(
                    name,
                    "BatchCopy requires a struct with named fields",
                ))
            }
        },
        _ => {
            return Err(syn::Error::new_spanned(
                name,
                "BatchCopy can only be derived for structs",
            ))
        }
    };

    let field_names: Vec<String> = fields
        .iter()
        .map(|f| f.ident.as_ref().unwrap().to_string())
        .collect();

    let columns = field_names.join(", ");
    let check_stmt = format!("SELECT {} FROM {} LIMIT 0", columns, table_name);
    let copy_stmt = format!(
        "COPY {} ({}) FROM STDIN (FORMAT binary)",
        table_name, columns
    );

    let pg_types: Vec<TokenStream2> = fields
        .iter()
        .map(field_pg_type)
        .collect::<syn::Result<_>>()?;

    let field_idents: Vec<&Ident> = fields
        .iter()
        .map(|f| f.ident.as_ref().unwrap())
        .collect();

    let boxes = field_idents.iter().map(|id| {
        quote! { ::std::boxed::Box::from(&self.#id) }
    });

    Ok(quote! {
        impl ::batch_copy::BatchCopyRow for #name {
            const CHECK_STATEMENT: &'static str = #check_stmt;
            const COPY_STATEMENT: &'static str = #copy_stmt;
            const TYPES: &'static [::batch_copy::__private::Type] = &[
                #(#pg_types),*
            ];
            fn binary_copy_vec(&self) -> ::std::vec::Vec<::std::boxed::Box<dyn ::batch_copy::__private::ToSql + Sync + Send + '_>> {
                vec![#(#boxes),*]
            }
        }
    })
}

fn extract_table_name(input: &DeriveInput) -> syn::Result<String> {
    for attr in &input.attrs {
        if attr.path().is_ident("batch_copy") {
            let nested =
                attr.parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)?;
            for meta in nested {
                if let Meta::NameValue(nv) = meta {
                    if nv.path.is_ident("table") {
                        if let Expr::Lit(ExprLit {
                            lit: Lit::Str(s), ..
                        }) = nv.value
                        {
                            return Ok(s.value());
                        }
                    }
                }
            }
        }
    }
    Ok(to_snake_case(&input.ident.to_string()))
}

fn to_snake_case(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 4);
    for (i, ch) in s.chars().enumerate() {
        if ch.is_uppercase() && i > 0 {
            out.push('_');
        }
        out.extend(ch.to_lowercase());
    }
    out
}

fn field_pg_type(field: &syn::Field) -> syn::Result<TokenStream2> {
    for attr in &field.attrs {
        if attr.path().is_ident("pg") {
            let variant = attr.parse_args::<Ident>()?;
            return Ok(quote! { ::batch_copy::__private::Type::#variant });
        }
    }

    infer_pg_type(&field.ty).ok_or_else(|| {
        syn::Error::new_spanned(
            &field.ty,
            "cannot infer PostgreSQL type for this field; annotate it with #[pg(TYPE)], \
             e.g. #[pg(TIMESTAMPTZ)], #[pg(JSONB)], #[pg(UUID)]",
        )
    })
}

fn infer_pg_type(ty: &Type) -> Option<TokenStream2> {
    match ty {
        Type::Path(tp) => {
            let last = tp.path.segments.last()?;
            let name = last.ident.to_string();

            if name == "Option" {
                if let PathArguments::AngleBracketed(ab) = &last.arguments {
                    if let Some(GenericArgument::Type(inner)) = ab.args.first() {
                        return infer_pg_type(inner);
                    }
                }
                return None;
            }

            if name == "Vec" {
                if let PathArguments::AngleBracketed(ab) = &last.arguments {
                    if let Some(GenericArgument::Type(Type::Path(inner))) = ab.args.first() {
                        if inner.path.is_ident("u8") {
                            return Some(
                                quote! { ::batch_copy::__private::Type::BYTEA },
                            );
                        }
                    }
                }
                return None;
            }

            Some(match name.as_str() {
                "bool" => quote! { ::batch_copy::__private::Type::BOOL },
                "i8" | "i16" => quote! { ::batch_copy::__private::Type::INT2 },
                "i32" => quote! { ::batch_copy::__private::Type::INT4 },
                "i64" => quote! { ::batch_copy::__private::Type::INT8 },
                "f32" => quote! { ::batch_copy::__private::Type::FLOAT4 },
                "f64" => quote! { ::batch_copy::__private::Type::FLOAT8 },
                "String" => quote! { ::batch_copy::__private::Type::TEXT },
                "NaiveDate" => quote! { ::batch_copy::__private::Type::DATE },
                "NaiveTime" => quote! { ::batch_copy::__private::Type::TIME },
                "NaiveDateTime" => quote! { ::batch_copy::__private::Type::TIMESTAMP },
                "DateTime" => quote! { ::batch_copy::__private::Type::TIMESTAMPTZ },
                "Uuid" => quote! { ::batch_copy::__private::Type::UUID },
                _ => return None,
            })
        }
        Type::Reference(tr) => {
            if let Type::Path(inner) = tr.elem.as_ref() {
                if inner.path.is_ident("str") {
                    return Some(quote! { ::batch_copy::__private::Type::TEXT });
                }
            }
            None
        }
        _ => None,
    }
}
