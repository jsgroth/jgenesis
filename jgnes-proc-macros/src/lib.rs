use chrono::Utc;
use proc_macro::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields};

/// This macro is fairly specific to the Mapper enum in jgnes-core, although it could theoretically
/// be more generalized if needed.
///
/// This macro is meant for use with enums in which every variant has exactly one field, and those
/// fields have different concrete types but extremely similar APIs (e.g. perhaps they all implement
/// a given trait).
///
/// It generates a declarative macro called `match_each_variant` that generates an enum match
/// expression and always takes three parameters: the value to match on, the identifier to bind the
/// single unnamed field to, and an expression to use as the match arm for every variant.
///
/// Example usage:
/// ```
/// use jgnes_proc_macros::MatchEachVariantMacro;
///
/// #[derive(MatchEachVariantMacro)]
/// enum Example {
///     VariantA(u16),
///     VariantB(u32),
///     VariantC(u64),
/// }
///
/// impl Example {
///     fn add_20(&self) -> u64 {
///         match_each_variant!(*self, number => u64::from(number) + 20)
///     }
/// }
///
/// assert_eq!(25_u64, Example::VariantA(5).add_20());
/// assert_eq!(30_u64, Example::VariantB(10).add_20());
/// assert_eq!(35_u64, Example::VariantC(15).add_20());
/// ```
///
/// The macro can optionally wrap the match arm expression in the variant constructor by using the
/// `:variant` marker:
/// ```
/// use jgnes_proc_macros::MatchEachVariantMacro;
///
/// #[derive(Debug, PartialEq, Eq, MatchEachVariantMacro)]
/// enum Example {
///     VariantA(u16),
///     VariantB(u32),
/// }
///
/// impl Example {
///     fn add_20(&self) -> Self {
///         match_each_variant!(*self, number => :variant(number + 20))
///     }
/// }
///
/// assert_eq!(Example::VariantA(25), Example::VariantA(5).add_20());
/// assert_eq!(Example::VariantB(120), Example::VariantB(100).add_20());
/// ```
///
/// # Panics
///
/// This macro will panic if applied to a struct, a union, or an enum in which not every variant
/// has exactly one unnamed field.
#[proc_macro_derive(MatchEachVariantMacro)]
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

/// Implement the `std::fmt::Display` trait for the given enum. Only supports enums which have only
/// fieldless variants.
///
/// # Panics
///
/// This macro will panic if applied to a struct, a union, or an enum with any variants that have
/// fields.
#[proc_macro_derive(EnumDisplay)]
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
                Self::#variant_name => write!(f, #variant_name_str)
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

/// Implement the `std::str::FromStr` trait for the given enum, with `FromStr::Err` set to `String`.
/// Only supports enums which have only fieldless variants. The generated implementation will be
/// case-insensitive.
///
/// # Panics
///
/// This macro will panic if applied to a struct, a union, or an enum with any variants that have
/// fields.
#[proc_macro_derive(EnumFromStr)]
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
                #variant_name_lowercase => Ok(Self::#variant_name)
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
                    _ => Err(format!(#err_fmt_string, s))
                }
            }
        }
    };

    gen.into()
}

/// Generate a string literal representing the timestamp that the code was built, in UTC.
///
/// The timestamp is regenerated each time this macro is called, so if it is used in multiple
/// locations then the value should be stored in a constant instead of calling the macro multiple
/// times.
///
/// Example usage:
/// ```
/// use jgnes_proc_macros::build_time_pretty_str;
///
/// // Explicit type only present for clarity; not required for use
/// let build_time: &'static str = build_time_pretty_str!();
/// println!("{build_time}");
/// ```
#[proc_macro]
pub fn build_time_pretty_str(_input: TokenStream) -> TokenStream {
    let now = Utc::now();
    let now_str = now.format("%B %-d, %Y %H:%M:%S UTC").to_string();

    let gen = quote! {
        #now_str
    };

    gen.into()
}
