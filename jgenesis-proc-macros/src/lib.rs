use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_quote, Data, DeriveInput, Type};

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
/// Generics example / test case:
/// ```
/// use jgenesis_proc_macros::ConfigDisplay;
///
/// #[derive(ConfigDisplay)]
/// struct Config<T> {
///     field: T,
/// }
///
/// let config = Config { field: String::from("hello") };
/// let s = format!("config: {config}");
/// assert_eq!(s, "config: \n  field: hello");
/// ```
///
/// # Panics
///
/// This macro only supports structs with named fields and it will panic if applied to any other
/// data type, including structs with no fields.
#[proc_macro_derive(ConfigDisplay, attributes(debug_fmt, indent_nested))]
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

/// Implements the `bincode::Encode` trait fpr the given type, with a fake implementation that
/// does not encode anything and always returns `Ok(())`.
///
/// # Panics
///
/// This macro will panic only if it is unable to parse its input.
#[proc_macro_derive(FakeEncode)]
pub fn fake_encode(input: TokenStream) -> TokenStream {
    let input: DeriveInput = syn::parse(input).expect("Unable to parse input");

    let type_ident = &input.ident;
    let gen = quote! {
        impl ::bincode::Encode for #type_ident {
            fn encode<E: ::bincode::enc::Encoder>(
                &self,
                _encoder: &mut E
            ) -> ::std::result::Result<(), ::bincode::error::EncodeError> {
                ::std::result::Result::Ok(())
            }
        }
    };

    gen.into()
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
    let input: DeriveInput = syn::parse(input).expect("Unable to parse input");

    let type_ident = &input.ident;
    let gen = quote! {
        impl ::bincode::Decode for #type_ident {
            fn decode<D: ::bincode::de::Decoder>(
                _decoder: &mut D
            ) -> ::std::result::Result<Self, ::bincode::error::DecodeError> {
                ::std::result::Result::Ok(Self::default())
            }
        }

        impl<'de> ::bincode::BorrowDecode<'de> for #type_ident {
            fn borrow_decode<D: ::bincode::de::BorrowDecoder<'de>>(
                _decoder: &mut D
            ) -> ::std::result::Result<Self, ::bincode::error::DecodeError> {
                ::std::result::Result::Ok(Self::default())
            }
        }
    };

    gen.into()
}
