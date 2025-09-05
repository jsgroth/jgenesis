use proc_macro::TokenStream;
use proc_macro2::{Ident, Span};
use quote::{format_ident, quote};
use syn::{DeriveInput, GenericParam, Lifetime, LifetimeParam, TypeParam};

pub fn fake_encode(input: TokenStream) -> TokenStream {
    let input: DeriveInput = syn::parse(input).expect("Unable to parse input");

    let (impl_generics, type_generics, where_clause) = input.generics.split_for_impl();

    let type_ident = &input.ident;
    let expanded = quote! {
        impl #impl_generics ::bincode::Encode for #type_ident #type_generics #where_clause {
            fn encode<E: ::bincode::enc::Encoder>(
                &self,
                _encoder: &mut E
            ) -> ::std::result::Result<(), ::bincode::error::EncodeError> {
                ::std::result::Result::Ok(())
            }
        }
    };

    expanded.into()
}

pub fn fake_decode(input: TokenStream) -> TokenStream {
    let input: DeriveInput = syn::parse(input).expect("Unable to parse input");

    let (_, type_generics, where_clause) = input.generics.split_for_impl();

    let mut decode_generics = input.generics.clone();
    decode_generics.params.insert(0, GenericParam::Type(TypeParam::from(format_ident!("Context"))));
    let (decode_impl_generics, _, _) = decode_generics.split_for_impl();

    let mut borrow_generics = input.generics.clone();
    borrow_generics
        .params
        .insert(0, GenericParam::Type(TypeParam::from(Ident::new("Context", Span::call_site()))));
    borrow_generics.params.insert(
        0,
        GenericParam::Lifetime(LifetimeParam::new(Lifetime::new("'de", Span::call_site()))),
    );
    let (borrow_impl_generics, _, _) = borrow_generics.split_for_impl();

    let type_ident = &input.ident;
    let expanded = quote! {
        impl #decode_impl_generics ::bincode::Decode<Context> for #type_ident #type_generics #where_clause {
            fn decode<D: ::bincode::de::Decoder<Context = Context>>(
                _decoder: &mut D
            ) -> ::std::result::Result<Self, ::bincode::error::DecodeError> {
                ::std::result::Result::Ok(Self::default())
            }
        }

        impl #borrow_impl_generics ::bincode::BorrowDecode<'de, Context> for #type_ident #type_generics #where_clause {
            fn borrow_decode<D: ::bincode::de::BorrowDecoder<'de, Context = Context>>(
                _decoder: &mut D
            ) -> ::std::result::Result<Self, ::bincode::error::DecodeError> {
                ::std::result::Result::Ok(Self::default())
            }
        }
    };

    expanded.into()
}
