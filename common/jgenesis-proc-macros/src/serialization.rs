use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{Field, ItemStruct};

pub fn deserialize_default_on_error(input: TokenStream) -> TokenStream {
    let Ok(input) = syn::parse::<ItemStruct>(input) else {
        panic!("macro can only be applied to struct definitions");
    };

    assert!(
        input.generics.params.is_empty(),
        "macro does not support structs with generic parameters"
    );

    let ItemStruct { attrs: struct_attrs, vis: struct_vis, ident: struct_ident, fields, .. } =
        &input;

    let mut deserialize_fn_definitions = Vec::new();
    let mut new_fields = Vec::new();

    for field in fields {
        let Some(field_ident) = &field.ident else {
            panic!("macro does not support structs with unnamed fields");
        };

        let Field { attrs: field_attrs, vis: field_vis, ty: field_ty, .. } = &field;

        let deserialize_fn_ident =
            format_ident!("__deserialize_{struct_ident}_{field_ident}_default_on_error");

        deserialize_fn_definitions.push(quote! {
            fn #deserialize_fn_ident<'de, D>(deserializer: D) -> ::std::result::Result<#field_ty, D::Error>
            where
                D: ::serde::Deserializer<'de>,
            {
                let value = <#field_ty as ::serde::Deserialize<'de>>::deserialize(deserializer)
                    .unwrap_or_else(|err| {
                        ::log::error!("error deserializing field '{}': {err}", ::std::stringify!(#field_ident));
                        <#struct_ident as ::std::default::Default>::default().#field_ident
                    });

                ::std::result::Result::Ok(value)
            }
        });

        let deserialize_fn_ident_str = deserialize_fn_ident.to_string();
        new_fields.push(quote! {
            #(#field_attrs)*
            #[serde(deserialize_with = #deserialize_fn_ident_str)]
            #field_vis #field_ident: #field_ty,
        });
    }

    quote! {
        #(#deserialize_fn_definitions)*

        #(#struct_attrs)*
        #struct_vis struct #struct_ident {
            #(#new_fields)*
        }
    }
    .into()
}
