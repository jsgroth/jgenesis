use proc_macro::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields};

pub fn enum_display(input: TokenStream) -> TokenStream {
    let ast: DeriveInput = syn::parse(input).expect("unable to parse input");

    let name = &ast.ident;

    let Data::Enum(data) = &ast.data else {
        panic!("EnumDisplay derive macro can only be applied to enums; {name} is not an enum");
    };

    let match_arms: Vec<_> = data
        .variants
        .iter()
        .map(|variant| {
            let variant_name = &variant.ident;
            assert!(variant.fields.is_empty(), "EnumDisplay macro only supports enums with only fieldless variants; {name}::{variant_name} has fields");

            let variant_name_str = variant_name.to_string();
            quote! {
                Self::#variant_name => ::std::write!(f, #variant_name_str)
            }
        })
        .collect();

    let gen = quote! {
        impl ::std::fmt::Display for #name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                match self {
                    #(#match_arms,)*
                }
            }
        }
    };

    gen.into()
}

pub fn enum_from_str(input: TokenStream) -> TokenStream {
    let ast: DeriveInput = syn::parse(input).expect("unable to parse input");

    let name = &ast.ident;

    let Data::Enum(data) = &ast.data else {
        panic!("EnumFromStr derive macro can only be applied to enums; {name} is not an enum");
    };

    let match_arms: Vec<_> = data
        .variants
        .iter()
        .map(|variant| {
            let variant_name = &variant.ident;
            assert!(variant.fields.is_empty(), "EnumFromStr macro only supports enums with only fieldless variants; {name}::{variant_name} has fields");

            let variant_name_lowercase = variant_name.to_string().to_ascii_lowercase();
            quote! {
                #variant_name_lowercase => ::std::result::Result::Ok(Self::#variant_name)
            }
        })
        .collect();

    let err_fmt_string = format!("invalid {name} string: '{{}}'");
    let gen = quote! {
        impl ::std::str::FromStr for #name {
            type Err = ::std::string::String;

            fn from_str(s: &str) -> ::std::result::Result<Self, Self::Err> {
                match s.to_ascii_lowercase().as_str() {
                    #(#match_arms,)*
                    _ => ::std::result::Result::Err(::std::format!(#err_fmt_string, s))
                }
            }
        }
    };

    gen.into()
}

pub fn enum_all(input: TokenStream) -> TokenStream {
    let input: DeriveInput = syn::parse(input).expect("Unable to parse input");

    let type_ident = &input.ident;
    let Data::Enum(data) = &input.data else {
        panic!("EnumAll only supports enums; {type_ident} is not an enum");
    };

    let variant_constructors: Vec<_> = data.variants.iter().map(|variant| {
        let variant_ident = &variant.ident;
        assert!(
            matches!(variant.fields, Fields::Unit),
            "EnumAll only supports enums with fieldless variants; {type_ident}::{variant_ident} is not a fieldless variant",
        );

        quote! {
            Self::#variant_ident
        }
    }).collect();

    let num_variants = data.variants.len();
    let gen = quote! {
        impl #type_ident {
            pub const ALL: [Self; #num_variants] = [#(#variant_constructors,)*];
        }
    };

    gen.into()
}

pub fn match_each_variant_macro(input: TokenStream) -> TokenStream {
    let input: DeriveInput = syn::parse(input).expect("Unable to parse input");

    let ident = &input.ident;

    let Data::Enum(data) = &input.data else {
        panic!("{ident} is not an enum");
    };

    let match_arms: Vec<_> = data
        .variants
        .iter()
        .map(|variant| {
            let variant_ident = &variant.ident;

            let Fields::Unnamed(fields) = &variant.fields else {
                panic!("{ident}::{variant_ident} should have unnamed fields");
            };

            assert_eq!(
                fields.unnamed.len(),
                1,
                "{ident}::{variant_ident} has {} unnamed fields, expected 1",
                fields.unnamed.len()
            );

            quote! {
                #ident::#variant_ident($field) => $match_arm
            }
        })
        .collect();

    let variant_match_arms: Vec<_> = data
        .variants
        .iter()
        .map(|variant| {
            let variant_ident = &variant.ident;

            // No need to re-validate the enum variants, that was done above

            quote! {
                #ident::#variant_ident($field) => #ident::#variant_ident($match_arm)
            }
        })
        .collect();

    let gen = quote! {
        macro_rules! match_each_variant {
            ($value:expr, $field:ident => $match_arm:expr) => {
                match $value {
                    #(#match_arms,)*
                }
            };
            ($value:expr, $field:ident => :variant($match_arm:expr)) => {
                match $value {
                    #(#variant_match_arms,)*
                }
            };
        }
    };

    gen.into()
}
