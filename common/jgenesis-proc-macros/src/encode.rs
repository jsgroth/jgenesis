use proc_macro::TokenStream;
use quote::quote;
use syn::DeriveInput;

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
