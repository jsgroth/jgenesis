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

    let Data::Enum(data) = &input.data
    else {
        panic!("{ident} is not an enum");
    };

    let match_arms: Vec<_> = data
        .variants
        .iter()
        .map(|variant| {
            let variant_ident = &variant.ident;

            let Fields::Unnamed(fields) = &variant.fields
            else {
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
