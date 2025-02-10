use proc_macro::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Field, Type, parse_quote};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CfgDisplayAttr {
    DebugFormat,
    IndentNested,
    Skip,
    Path,
}

fn parse_cfg_display_attrs(field: &Field) -> Vec<CfgDisplayAttr> {
    let Some(cfg_display_attr) =
        field.attrs.iter().find(|attr| attr.path().is_ident("cfg_display"))
    else {
        return vec![];
    };

    let mut attrs = Vec::new();
    cfg_display_attr
        .parse_nested_meta(|meta| {
            if meta.path.is_ident("debug_fmt") {
                attrs.push(CfgDisplayAttr::DebugFormat);
            } else if meta.path.is_ident("indent_nested") {
                attrs.push(CfgDisplayAttr::IndentNested);
            } else if meta.path.is_ident("skip") {
                attrs.push(CfgDisplayAttr::Skip);
            } else if meta.path.is_ident("path") {
                attrs.push(CfgDisplayAttr::Path);
            } else {
                return Err(meta.error("Invalid cfg_display meta"));
            }

            Ok(())
        })
        .expect("Failed to parse cfg_display field attribute");

    attrs
}

pub fn config_display(input: TokenStream) -> TokenStream {
    let input: DeriveInput = syn::parse(input).expect("Unable to parse input");

    let Data::Struct(struct_data) = input.data else {
        panic!("ConfigDisplay derive macro only applies to structs");
    };

    let fields: Vec<_> = struct_data
        .fields
        .iter()
        .map(|field| (field, parse_cfg_display_attrs(field)))
        .filter(|(_, attrs)| !attrs.contains(&CfgDisplayAttr::Skip))
        .collect();

    assert!(!fields.is_empty(), "ConfigDisplay derive macro only applies to structs with fields");

    let writeln_statements: Vec<_> = fields
        .iter()
        .enumerate()
        .map(|(i, (field, attrs))| {
            let Some(field_ident) = &field.ident else {
                panic!("ConfigDisplay derive macro only supports structs with named fields");
            };

            let debug_fmt = attrs.contains(&CfgDisplayAttr::DebugFormat);
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
            let is_path = attrs.contains(&CfgDisplayAttr::Path);

            let format_invocation = if is_option {
                let none_str = format!("  {field_ident}: <None>");

                let field_display = if is_path {
                    quote! { f.display() }
                } else {
                    quote! { f }
                };

                quote! {
                    self.#field_ident.as_ref().map(|f| format!(#fmt_string, #field_display))
                        .unwrap_or_else(|| #none_str.into())
                }
            } else if is_path {
                quote! {
                    format!(#fmt_string, self.#field_ident.display())
                }
            } else {
                quote! {
                    format!(#fmt_string, self.#field_ident)
                }
            };

            let indent_nested = attrs.contains(&CfgDisplayAttr::IndentNested);
            let format_invocation = if indent_nested {
                quote! {
                    #format_invocation.replace("\n  ", "\n    ")
                }
            } else {
                format_invocation
            };

            if i == fields.len() - 1 {
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
