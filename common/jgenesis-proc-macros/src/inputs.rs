#![allow(clippy::manual_assert)]

use proc_macro2::{Ident, TokenStream};
use quote::{format_ident, quote};
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{Field, Token, Type, Variant, braced, parse_macro_input};

struct ButtonEnum {
    name: Ident,
    variants: Punctuated<Variant, Token![,]>,
}

impl Parse for ButtonEnum {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        input.parse::<Token![enum]>()?;

        let name: Ident = input.parse()?;

        let content;
        braced!(content in input);

        let variants = content.parse_terminated(Variant::parse, Token![,])?;

        Ok(Self { name, variants })
    }
}

struct JoypadStruct {
    name: Ident,
}

impl Parse for JoypadStruct {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        input.parse::<Token![struct]>()?;

        let name: Ident = input.parse()?;

        let content;
        braced!(content in input);

        let filler_ident: Ident = content.parse()?;
        if filler_ident.to_string().as_str() != "buttons" {
            return Err(content.error(format!("Expected 'buttons', got '{filler_ident}'")));
        }

        content.parse::<Token![!]>()?;

        Ok(Self { name })
    }
}

enum InputsField {
    Player(Ident),
    Button(Ident),
}

struct InputsStruct {
    name: Ident,
    fields: Vec<(Ident, InputsField)>,
}

impl Parse for InputsStruct {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        input.parse::<Token![struct]>()?;

        let name: Ident = input.parse()?;

        let content;
        braced!(content in input);

        let mut fields = Vec::new();
        let raw_fields = content.parse_terminated(Field::parse_named, Token![,])?;
        for raw_field in raw_fields {
            let field_name = raw_field.ident.unwrap();
            let Type::Path(path) = raw_field.ty else {
                return Err(content.error("Expected type path"));
            };

            let segments: Vec<_> = path.path.segments.into_iter().collect();
            if segments.len() != 2 {
                return Err(content
                    .error(format!("Expected type path of length 2, was {}", segments.len())));
            }

            let field = match segments[0].ident.to_string().as_str() {
                "Player" => InputsField::Player(segments[1].ident.clone()),
                "Button" => InputsField::Button(segments[1].ident.clone()),
                _ => {
                    return Err(content.error(format!(
                        "Expected 'Player' or 'Button', got '{}'",
                        segments[0].ident
                    )));
                }
            };
            fields.push((field_name, field));
        }

        Ok(Self { name, fields })
    }
}

struct MacroInput {
    button_enum: ButtonEnum,
    joypad_struct: JoypadStruct,
    inputs_struct: Option<InputsStruct>,
}

impl Parse for MacroInput {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let button_enum: ButtonEnum = input.parse()?;
        let joypad_struct: JoypadStruct = input.parse()?;

        if input.is_empty() {
            return Ok(MacroInput { button_enum, joypad_struct, inputs_struct: None });
        }

        let inputs_struct: InputsStruct = input.parse()?;

        Ok(Self { button_enum, joypad_struct, inputs_struct: Some(inputs_struct) })
    }
}

pub fn define_controller_inputs(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let input = parse_macro_input!(input as MacroInput);

    let button_enum = generate_button_enum(&input.button_enum);

    let joypad_struct = generate_joypad_struct(&input.button_enum, &input.joypad_struct);

    let inputs_struct = match &input.inputs_struct {
        Some(inputs_struct) => {
            generate_inputs_struct(&input.button_enum, &input.joypad_struct, inputs_struct)
        }
        None => quote! {},
    };

    let gen = quote! {
        #button_enum
        #joypad_struct
        #inputs_struct
    };

    gen.into()
}

fn generate_button_enum(button_enum: &ButtonEnum) -> TokenStream {
    let name = &button_enum.name;

    let variants: Vec<_> = button_enum
        .variants
        .iter()
        .map(|variant| {
            let ident = &variant.ident;
            quote! {
                #ident
            }
        })
        .collect();

    quote! {
        #[derive(
            Debug,
            Clone,
            Copy,
            PartialEq,
            Eq,
            ::bincode::Encode,
            ::bincode::Decode,
            ::jgenesis_proc_macros::EnumDisplay,
            ::jgenesis_proc_macros::EnumFromStr,
            ::jgenesis_proc_macros::EnumAll,
        )]
        pub enum #name {
            #(#variants,)*
        }
    }
}

fn generate_joypad_struct(button_enum: &ButtonEnum, joypad_struct: &JoypadStruct) -> TokenStream {
    let button_name = &button_enum.name;
    let joypad_name = &joypad_struct.name;

    let mut fields = Vec::new();
    let mut match_arms = Vec::new();
    for variant in &button_enum.variants {
        if variant.attrs.iter().any(|attr| attr.path().is_ident("on_console")) {
            // SMS/GG: Joypad struct should not contain any buttons with the #[on_console] attribute
            continue;
        }

        let field_name = format_ident!("{}", variant.ident.to_string().to_lowercase());
        fields.push(quote! {
            pub #field_name: bool
        });

        let variant_ident = &variant.ident;
        match_arms.push(quote! {
            #button_name::#variant_ident => self.#field_name = pressed
        });
    }

    match_arms.push(quote! {
        _ => return
    });

    quote! {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ::bincode::Encode, ::bincode::Decode)]
        pub struct #joypad_name {
            #(#fields,)*
        }

        impl #joypad_name {
            #[inline]
            pub fn set_button(&mut self, button: #button_name, pressed: bool) {
                match button {
                    #(#match_arms,)*
                }
            }

            #[inline]
            pub fn with_button(mut self, button: #button_name, pressed: bool) -> Self {
                self.set_button(button, pressed);
                self
            }
        }
    }
}

fn generate_inputs_struct(
    button_enum: &ButtonEnum,
    joypad_struct: &JoypadStruct,
    inputs_struct: &InputsStruct,
) -> TokenStream {
    let button_name = &button_enum.name;
    let joypad_name = &joypad_struct.name;
    let inputs_name = &inputs_struct.name;

    let mut fields = Vec::new();
    let mut button_match_arms = Vec::new();
    let mut player_match_arms = Vec::new();
    for (field_name, raw_field) in &inputs_struct.fields {
        match raw_field {
            InputsField::Player(player) => {
                fields.push(quote! {
                    pub #field_name: #joypad_name
                });

                player_match_arms.push(quote! {
                    (::jgenesis_common::input::Player::#player, _) => self.#field_name.set_button(button, pressed)
                });
            }
            InputsField::Button(button) => {
                fields.push(quote! {
                    pub #field_name: bool
                });

                button_match_arms.push(quote! {
                    (_, #button_name::#button) => self.#field_name = pressed
                });
            }
        }
    }

    quote! {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ::bincode::Encode, ::bincode::Decode)]
        pub struct #inputs_name {
            #(#fields,)*
        }

        impl #inputs_name {
            #[inline]
            pub fn set_button(&mut self, button: #button_name, player: ::jgenesis_common::input::Player, pressed: bool) {
                match (player, button) {
                    #(#button_match_arms,)*
                    #(#player_match_arms,)*
                }
            }

            #[inline]
            pub fn with_button(mut self, button: #button_name, player: ::jgenesis_common::input::Player, pressed: bool) -> Self {
                self.set_button(button, player, pressed);
                self
            }
        }
    }
}
