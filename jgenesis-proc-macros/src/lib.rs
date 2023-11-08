use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{parse_quote, Data, DataEnum, DataStruct, DeriveInput, Field, Fields, Type};

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

/// On an enum with only fieldless variants, add an `ALL` constant of type `[Self; N]` that contains
/// every variant of the enum. The variant order in `ALL` will equal the variant declaration order.
///
/// Examples:
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
/// ```
/// use jgenesis_proc_macros::EnumAll;
///
/// #[derive(Debug, PartialEq, EnumAll)]
/// enum Unit {}
///
/// assert_eq!(Unit::ALL, []);
/// ```
///
/// # Panics
///
/// This macro will panic if applied to a struct, a union, or an enum with non-fieldless variants.
#[proc_macro_derive(EnumAll)]
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
/// Examples/tests:
/// ```
/// use jgenesis_common::frontend::PartialClone;
///
/// #[derive(Debug, PartialEq, PartialClone)]
/// struct UnitStruct;
///
/// assert_eq!(UnitStruct, UnitStruct.partial_clone());
/// ```
///
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
/// ```
/// use jgenesis_common::frontend::PartialClone;
///
/// #[derive(Debug, PartialEq, PartialClone)]
/// struct Nested {
///     a: Vec<u8>,
///     #[partial_clone(default)]
///     b: Vec<u16>,
///     c: String,
/// }
///
/// #[derive(Debug, PartialEq, PartialClone)]
/// struct NamedFields {
///     d: Vec<u8>,
///     #[partial_clone(default)]
///     e: Vec<u16>,
///     #[partial_clone(partial)]
///     f: Nested,
/// }
///
/// let inner = Nested { a: vec![1, 2, 3], b: vec![4, 5, 6], c: "hello".into() };
/// let outer = NamedFields { d: vec![7, 8, 9], e: vec![10, 11, 12], f: inner };
///
/// let expected = NamedFields {
///     d: vec![7, 8, 9],
///     e: vec![],
///     f: Nested { a: vec![1, 2, 3], b: vec![], c: "hello".into() },
/// };
/// assert_eq!(outer.partial_clone(), expected);
/// ```
///
/// ```
/// use jgenesis_common::frontend::PartialClone;
///
/// #[derive(Debug, PartialEq, PartialClone)]
/// struct Nested(Vec<u8>, #[partial_clone(default)] String);
///
/// #[derive(Debug, PartialEq, PartialClone)]
/// struct GenericStruct<T>(#[partial_clone(partial)] T, Vec<u8>);
///
/// let inner = Nested(vec![1, 2, 3], "hello".into());
/// let outer = GenericStruct(inner, vec![4, 5, 6]);
///
/// let expected_inner = Nested(vec![1, 2, 3], String::new());
/// let expected_outer = GenericStruct(expected_inner, vec![4, 5, 6]);
/// assert_eq!(outer.partial_clone(), expected_outer);
/// ```
///
/// ```
/// use jgenesis_common::frontend::PartialClone;
///
/// #[derive(Debug, PartialEq, PartialClone)]
/// struct Nested(Vec<u8>, #[partial_clone(default)] String);
///
/// #[derive(Debug, PartialEq, PartialClone)]
/// enum Foo {
///     Unit,
///     Unnamed(Vec<u8>, #[partial_clone(default)] String, #[partial_clone(partial)] Nested),
///     Named {
///         a: Vec<u8>,
///         #[partial_clone(default)]
///         b: String,
///         #[partial_clone(partial)]
///         c: Nested,
///     },
/// }
///
/// assert_eq!(Foo::Unit, Foo::Unit.partial_clone());
///
/// let inner = Nested(vec![1, 2, 3], "hello".into());
/// let outer = Foo::Unnamed(vec![4, 5, 6], "world".into(), inner);
///
/// let expected_inner = Nested(vec![1, 2, 3], String::new());
/// let expected_outer = Foo::Unnamed(vec![4, 5, 6], String::new(), expected_inner);
/// assert_eq!(outer.partial_clone(), expected_outer);
/// ```
/// # Panics
///
/// This macro currently only supports structs and enums, and it will panic if applied to a union.
#[proc_macro_derive(PartialClone, attributes(partial_clone))]
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
