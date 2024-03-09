use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_quote, Data, DeriveInput, Type};

pub fn config_display(input: TokenStream) -> TokenStream {
    let input: DeriveInput = syn::parse(input).expect("Unable to parse input");

    let Data::Struct(struct_data) = input.data else {
        panic!("ConfigDisplay derive macro only applies to structs");
    };

    assert!(
        !struct_data.fields.is_empty(),
        "ConfigDisplay derive macro only applies to structs with fields"
    );

    let writeln_statements: Vec<_> = struct_data
        .fields
        .iter()
        .enumerate()
        .map(|(i, field)| {
            let Some(field_ident) = &field.ident else {
                panic!("ConfigDisplay derive macro only supports structs with named fields");
            };

            let debug_fmt = field.attrs.iter().any(|attr| attr.path().is_ident("debug_fmt"));
            let fmt_string = if debug_fmt {
                format!("  {field_ident}: {{:?}}")
            } else {
                format!("  {field_ident}: {{}}")
            };

            let is_option = match &field.ty {
                Type::Path(path) => {
                    let first_segment = path.path.segments.iter().next();
                    first_segment.is_some_and(|segment| segment.ident == "Option")
                }
                _ => false,
            };
            let format_invocation = if is_option {
                let none_str = format!("  {field_ident}: <None>");
                quote! {
                    self.#field_ident.as_ref().map(|f| format!(#fmt_string, f))
                        .unwrap_or_else(|| #none_str.into())
                }
            } else {
                quote! {
                    format!(#fmt_string, self.#field_ident)
                }
            };

            let indent_nested =
                field.attrs.iter().any(|attr| attr.path().is_ident("indent_nested"));
            let format_invocation = if indent_nested {
                quote! {
                    #format_invocation.replace("\n  ", "\n    ")
                }
            } else {
                format_invocation
            };

            if i == struct_data.fields.len() - 1 {
                quote! {
                    ::std::write!(f, "{}", #format_invocation)
                }
            } else {
                quote! {
                    ::std::writeln!(f, "{}", #format_invocation)?;
                }
            }
        })
        .collect();

    let mut generics = input.generics.clone();
    for type_param in generics.type_params_mut() {
        type_param.bounds.push(parse_quote!(::std::fmt::Display));
        type_param.bounds.push(parse_quote!(::std::fmt::Debug));
    }
    let (impl_generics, type_generics, where_clause) = generics.split_for_impl();

    let struct_ident = &input.ident;
    let gen = quote! {
        impl #impl_generics ::std::fmt::Display for #struct_ident #type_generics #where_clause {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                ::std::writeln!(f)?;
                #(#writeln_statements)*
            }
        }
    };

    gen.into()
}
