mod config;
mod encode;
mod enums;
mod partialclone;

use proc_macro::TokenStream;

/// Implement the `std::fmt::Display` trait for the given enum. Only supports enums which have only
/// fieldless variants.
///
/// This macro also implements a method `to_str(&self) -> &'static str` for cases where an owned
/// String is not needed.
///
/// # Panics
///
/// This macro will panic if applied to a struct, a union, or an enum with any variants that have
/// fields.
#[proc_macro_derive(EnumDisplay)]
pub fn enum_display(input: TokenStream) -> TokenStream {
    enums::enum_display(input)
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
    enums::enum_from_str(input)
}

/// On an enum with only fieldless variants, add an `ALL` constant of type `[Self; N]` that contains
/// every variant of the enum. The variant order in `ALL` will equal the variant declaration order.
///
/// Example:
/// ```
/// use jgenesis_proc_macros::EnumAll;
///
/// #[derive(Debug, PartialEq, EnumAll)]
/// enum Foo {
///     A,
///     B,
///     C,
/// }
///
/// // Explicit type for clarity
/// let expected: [Foo; 3] = [Foo::A, Foo::B, Foo::C];
/// assert_eq!(Foo::ALL, expected);
/// ```
///
/// # Panics
///
/// This macro will panic if applied to a struct, a union, or an enum with non-fieldless variants.
#[proc_macro_derive(EnumAll)]
pub fn enum_all(input: TokenStream) -> TokenStream {
    enums::enum_all(input)
}

/// Implement the `clap::ValueEnum` trait for a struct, using a custom implementation rather than
/// the one provided by `derive(clap::ValueEnum)`.
///
/// The implementation differs only in the string values generated. Where `derive(clap::ValueEnum)`
/// lowercases variant names and inserts a `-` character at every word break, this implementation
/// uses the variant name directly.
///
/// This macro requires that the [`EnumAll`] and [`EnumDisplay`] macros are also used.
#[proc_macro_derive(CustomValueEnum)]
pub fn custom_value_enum(input: TokenStream) -> TokenStream {
    enums::custom_value_enum(input)
}

/// Implement the `std::fmt::Display` trait for a struct, with an implementation meant for
/// pretty-printing configs. By default all fields are printed using their own `std::fmt::Display`
/// implementation.
///
/// For example:
/// ```
/// use jgenesis_proc_macros::ConfigDisplay;
///
/// #[derive(ConfigDisplay)]
/// struct Config {
///     foo: u32,
///     bar: String,
/// }
///
/// // This prints the following output:
/// // config:
/// //   foo: 5
/// //   bar: asdf
/// let s = format!("config: {}", Config { foo: 5, bar: "asdf".into() });
/// assert_eq!(s, "config: \n  foo: 5\n  bar: asdf".to_owned());
/// ```
///
/// The `#[debug_fmt]` attribute can be used to indicate that a field should be formatted using its
/// `std::fmt::Debug` implementation rather than its `std::fmt::Display` implementation. For example:
/// ```
/// use jgenesis_proc_macros::ConfigDisplay;
///
/// #[derive(Debug)]
/// struct NotDisplay(String);
///
/// #[derive(ConfigDisplay)]
/// struct Config {
///     bar: bool,
///     #[debug_fmt]
///     baz: NotDisplay,
/// }
///
/// // This prints the following output:
/// // config:
/// //   foo: Some(5)
/// //   bar: true
/// //   baz: NotDisplay("fdsa")
/// let config = Config {
///     bar: true,
///     baz: NotDisplay("fdsa".into()),
/// };
/// let s = format!("config: {config}");
/// assert_eq!(s, "config: \n  bar: true\n  baz: NotDisplay(\"fdsa\")");
/// ```
///
/// The `#[indent_nested]` attribute allows indenting when printing a field that also implements
/// the `Display` trait through this macro:
/// ```
/// use jgenesis_proc_macros::ConfigDisplay;
///
/// #[derive(ConfigDisplay)]
/// struct Inner {
///     foo: u32,
/// }
///
/// #[derive(ConfigDisplay)]
/// struct Outer {
///     bar: u32,
///     #[indent_nested]
///     inner: Inner,
///     baz: u32,
/// }
///
/// // This prints the following output:
/// // config:
/// //   bar: 3
/// //   inner:
/// //     foo: 4
/// //   baz: 5
/// let config = Outer {
///     bar: 3,
///     inner: Inner { foo: 4 },
///     baz: 5,
/// };
/// let s = format!("config: {config}");
/// assert_eq!(s, "config: \n  bar: 3\n  inner: \n    foo: 4\n  baz: 5");
/// ```
///
/// Options will automatically be formatted by unwrapping if the value is Some(v) and printing
/// the string "<None>" if the value is None:
/// ```
/// use jgenesis_proc_macros::ConfigDisplay;
///
/// #[derive(ConfigDisplay)]
/// struct Config {
///     a: Option<String>,
///     b: Option<u32>,
/// }
///
/// // This prints the following output:
/// // config:
/// //   a: hello
/// //   b: <None>
/// let config = Config { a: Some("hello".into()), b: None };
/// let s = format!("config: {config}");
/// assert_eq!(s, "config: \n  a: hello\n  b: <None>");
/// ```
///
/// # Panics
///
/// This macro only supports structs with named fields and it will panic if applied to any other
/// data type, including structs with no fields.
#[proc_macro_derive(
    ConfigDisplay,
    attributes(debug_fmt, indent_nested, cfg_display_skip, cfg_display_path)
)]
pub fn config_display(input: TokenStream) -> TokenStream {
    config::config_display(input)
}

/// Implements the `bincode::Encode` trait fpr the given type, with a fake implementation that
/// does not encode anything and always returns `Ok(())`.
///
/// # Panics
///
/// This macro will panic only if it is unable to parse its input.
#[proc_macro_derive(FakeEncode)]
pub fn fake_encode(input: TokenStream) -> TokenStream {
    encode::fake_encode(input)
}

/// Implements the `bincode::Decode` and `bincode::BorrowDecode` traits for the given type,
/// with fake implementations that do not decode anything and always return `Ok(Self::default())`.
///
/// The type must have a `default()` associated function, preferably through implementing the
/// `Default` trait.
///
/// # Panics
///
/// This macro will panic only if it is unable to parse its input.
#[proc_macro_derive(FakeDecode)]
pub fn fake_decode(input: TokenStream) -> TokenStream {
    encode::fake_decode(input)
}

/// Implement the `jgenesis_common::frontend::PartialClone` trait for a given struct or enum.
///
/// This macro should be imported through `jgenesis_common` instead of directly from this crate so
/// that both the macro and the trait are imported.
///
/// Fields that are not marked with a `#[partial_clone]` attribute will be cloned using that type's
/// implementation of the `Clone` trait.
///
/// Fields that are marked with `#[partial_clone(default)]` will not be cloned, and instead the
/// partial clone will contain the default value for that type (via the `Default` trait).
///
/// Fields that are marked with `#[partial_clone(partial)]` will be cloned using that type's
/// implementation of the `PartialClone` trait.
///
/// If the struct has any generic type parameters, the `PartialClone` trait will only be implemented
/// where all of the generic types implement `PartialClone`.
///
/// Example:
/// ```
/// use jgenesis_common::frontend::PartialClone;
///
/// #[derive(Debug, PartialEq, PartialClone)]
/// struct Nested(Vec<u8>, #[partial_clone(default)] Vec<u16>, String);
///
/// #[derive(Debug, PartialEq, PartialClone)]
/// struct UnnamedFields(Vec<u8>, #[partial_clone(default)] Vec<u16>, #[partial_clone(partial)] Nested);
///
/// let inner = Nested(vec![1, 2, 3], vec![4, 5, 6], "hello".into());
/// let outer = UnnamedFields(vec![7, 8, 9], vec![10, 11, 12], inner);
///
/// let expected = UnnamedFields(vec![7, 8, 9], vec![], Nested(vec![1, 2, 3], vec![], "hello".into()));
/// assert_eq!(outer.partial_clone(), expected);
/// ```
///
/// # Panics
///
/// This macro currently only supports structs and enums, and it will panic if applied to a union.
#[proc_macro_derive(PartialClone, attributes(partial_clone))]
pub fn partial_clone(input: TokenStream) -> TokenStream {
    partialclone::partial_clone(input)
}

/// This macro is fairly specific to the NES Mapper enum, although it could theoretically
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
/// use jgenesis_proc_macros::MatchEachVariantMacro;
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
/// use jgenesis_proc_macros::MatchEachVariantMacro;
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
    enums::match_each_variant_macro(input)
}
