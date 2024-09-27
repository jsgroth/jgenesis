use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{Data, DataEnum, DataStruct, DeriveInput, Field, Fields, parse_quote};

pub fn partial_clone(input: TokenStream) -> TokenStream {
    let input: DeriveInput = syn::parse(input).expect("Unable to parse input");

    let type_ident = &input.ident;
    let body = match &input.data {
        Data::Struct(data) => partial_clone_struct_body(data),
        Data::Enum(data) => partial_clone_enum_body(data),
        Data::Union(_) => panic!("PartialClone does not support unions; {type_ident} is a union"),
    };

    let mut generics = input.generics.clone();
    for type_param in generics.type_params_mut() {
        type_param.bounds.push(parse_quote!(::jgenesis_common::frontend::PartialClone));
    }
    let (impl_generics, type_generics, where_clause) = generics.split_for_impl();

    let gen = quote! {
        impl #impl_generics ::jgenesis_common::frontend::PartialClone for #type_ident #type_generics #where_clause {
            fn partial_clone(&self) -> Self {
                #body
            }
        }
    };

    gen.into()
}

fn partial_clone_struct_body(data: &DataStruct) -> proc_macro2::TokenStream {
    match &data.fields {
        Fields::Unit => quote! { Self },
        Fields::Unnamed(fields) => {
            let constructor_fields: Vec<_> = fields
                .unnamed
                .iter()
                .enumerate()
                .map(|(i, field)| {
                    let i = syn::Index::from(i);
                    match parse_partial_clone_attr(field) {
                        PartialCloneAttr::None => quote! {
                            ::std::clone::Clone::clone(&self.#i)
                        },
                        PartialCloneAttr::PartialClone => quote! {
                            ::jgenesis_common::frontend::PartialClone::partial_clone(&self.#i)
                        },
                        PartialCloneAttr::Default => quote! {
                            ::std::default::Default::default()
                        },
                    }
                })
                .collect();

            quote! {
                Self(#(#constructor_fields,)*)
            }
        }
        Fields::Named(fields) => {
            let constructor_fields: Vec<_> = fields
                .named
                .iter()
                .map(|field| {
                    let field_ident =
                        field.ident.as_ref().expect("Nested inside Fields::Named match arm");
                    match parse_partial_clone_attr(field) {
                        PartialCloneAttr::None => quote! {
                            #field_ident: ::std::clone::Clone::clone(&self.#field_ident)
                        },
                        PartialCloneAttr::PartialClone => quote! {
                            #field_ident: ::jgenesis_common::frontend::PartialClone::partial_clone(&self.#field_ident)
                        },
                        PartialCloneAttr::Default => quote! {
                            #field_ident: ::std::default::Default::default()
                        },
                    }
                })
                .collect();

            quote! {
                Self {
                    #(#constructor_fields,)*
                }
            }
        }
    }
}

fn partial_clone_enum_body(data: &DataEnum) -> proc_macro2::TokenStream {
    let match_arms: Vec<_> = data.variants.iter().map(|variant| {
        let variant_ident = &variant.ident;
        match &variant.fields {
            Fields::Unit => quote! { Self::#variant_ident => Self::#variant_ident },
            Fields::Unnamed(fields) => {
                let (field_idents, field_constructors): (Vec<_>, Vec<_>) = fields.unnamed.iter().enumerate().map(|(i, field)| {
                    let partial_clone_attr = parse_partial_clone_attr(field);

                    let field_ident = match partial_clone_attr {
                        PartialCloneAttr::Default => format_ident!("_"),
                        _ => format_ident!("t{i}")
                    };

                    let field_constructor = match partial_clone_attr {
                        PartialCloneAttr::None => quote! {
                            ::std::clone::Clone::clone(#field_ident)
                        },
                        PartialCloneAttr::PartialClone => quote! {
                            ::jgenesis_common::frontend::PartialClone::partial_clone(#field_ident)
                        },
                        PartialCloneAttr::Default => quote! {
                            ::std::default::Default::default()
                        },
                    };

                    (field_ident, field_constructor)
                }).unzip();

                quote! {
                    Self::#variant_ident(#(#field_idents,)*) => Self::#variant_ident(#(#field_constructors,)*)
                }
            }
            Fields::Named(fields) => {
                let (field_bindings, field_constructors): (Vec<_>, Vec<_>) = fields.named.iter().map(|field| {
                    let partial_clone_attr = parse_partial_clone_attr(field);

                    let field_ident = &field.ident;

                    let field_binding = match partial_clone_attr {
                        PartialCloneAttr::Default => quote! { #field_ident: _ },
                        _ => quote! { #field_ident }
                    };

                    let field_constructor = match partial_clone_attr {
                        PartialCloneAttr::None => quote! {
                            #field_ident: ::std::clone::Clone::clone(#field_ident)
                        },
                        PartialCloneAttr::PartialClone => quote! {
                            #field_ident: ::jgenesis_common::frontend::PartialClone::partial_clone(#field_ident)
                        },
                        PartialCloneAttr::Default => quote! {
                            #field_ident: ::std::default::Default::default()
                        },
                    };

                    (field_binding, field_constructor)
                }).unzip();

                quote! {
                    Self::#variant_ident { #(#field_bindings,)* } => Self::#variant_ident { #(#field_constructors,)* }
                }
            }
        }
    }).collect();

    quote! {
        match self {
            #(#match_arms,)*
        }
    }
}

enum PartialCloneAttr {
    None,
    PartialClone,
    Default,
}

fn parse_partial_clone_attr(field: &Field) -> PartialCloneAttr {
    field.attrs.iter().find_map(|attr| {
        attr.path().is_ident("partial_clone").then(|| {
            let mut partial = false;
            let mut default = false;
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("partial") {
                    partial = true;
                    Ok(())
                } else if meta.path.is_ident("default") {
                    default = true;
                    Ok(())
                } else {
                    Err(meta.error("nested partial_clone attribute must be 'partial' or 'default'"))
                }
            }).expect("partial_clone attribute missing nested attribute of 'partial' or 'default'");

            if partial && default {
                panic!("partial_clone has both 'partial' and 'default' attributes, expected exactly one");
            } else if partial {
                PartialCloneAttr::PartialClone
            } else if default {
                PartialCloneAttr::Default
            } else {
                panic!("partial_clone attribute must have nested attribute of either 'partial' or 'default'");
            }
        })
    }).unwrap_or(PartialCloneAttr::None)
}
