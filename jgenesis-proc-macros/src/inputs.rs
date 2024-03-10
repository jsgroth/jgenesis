#![allow(clippy::manual_assert)]

use proc_macro2::{Delimiter, Group, Ident, TokenStream, TokenTree};
use quote::{format_ident, quote};
use std::collections::HashMap;

const BUTTON_IDENT: &str = "button_ident";
const JOYPAD_IDENT: &str = "joypad_ident";
const INPUTS_IDENT: &str = "inputs_ident";
const BUTTONS: &str = "buttons";
const CONSOLE_BUTTONS: &str = "console_buttons";
const INPUTS: &str = "inputs";

pub fn define_controller_inputs(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let input: TokenStream = input.into();

    let tokens: Vec<_> = input.into_iter().collect();
    let map = parse_map_tokens(&tokens);

    let (button_ident, buttons, console_buttons) = parse_button_fields(&map);
    let joypad_ident = parse_joypad_ident(&map);

    let button_enum = generate_button_enum(button_ident, &buttons, &console_buttons);
    let joypad_struct = generate_joypad_struct(button_ident, joypad_ident, &buttons);

    let inputs_ident = map.get(INPUTS_IDENT).map(|tt| {
        let TokenTree::Ident(ident) = tt else {
            panic!("Expected inputs identifier, got '{tt}'");
        };
        ident
    });

    let inputs_struct = inputs_ident.map_or(TokenStream::new(), |inputs_ident| {
        let Some(inputs_tt) = map.get(INPUTS) else {
            panic!("Missing '{INPUTS}' field, required if '{INPUTS_IDENT}' is present")
        };
        generate_inputs_struct(button_ident, joypad_ident, inputs_ident, inputs_tt)
    });

    let gen = quote! {
        #button_enum
        #joypad_struct
        #inputs_struct
    };
    gen.into()
}

fn parse_map_tokens(tokens: &[TokenTree]) -> HashMap<String, TokenTree> {
    let mut entries = Vec::new();
    parse_map_tokens_inner(tokens, &mut entries);
    entries.into_iter().collect()
}

fn parse_map_tokens_inner(tokens: &[TokenTree], entries: &mut Vec<(String, TokenTree)>) {
    if tokens.is_empty() {
        return;
    }

    if tokens.len() == 1 {
        panic!("Unexpected end of input: '{}'", tokens[0]);
    }

    if tokens.len() == 2 {
        panic!("Unexpected end of input: '{} {}'", tokens[0], tokens[1]);
    }

    if !is_punct_match(&tokens[1], ':') {
        panic!("Expected ':', got: '{}'", tokens[1]);
    }

    let TokenTree::Ident(ident) = &tokens[0] else { panic!("Unexpected token: '{}'", tokens[0]) };

    entries.push((ident.to_string(), tokens[2].clone()));
    let remaining_tokens = if tokens.len() > 3 && is_punct_match(&tokens[3], ',') {
        &tokens[4..]
    } else {
        &tokens[3..]
    };

    parse_map_tokens_inner(remaining_tokens, entries);
}

fn is_punct_match(token: &TokenTree, ch: char) -> bool {
    matches!(token, TokenTree::Punct(punct) if punct.as_char() == ch)
}

fn parse_button_fields(map: &HashMap<String, TokenTree>) -> (&Ident, Vec<Ident>, Vec<Ident>) {
    let Some(button_ident_tt) = map.get(BUTTON_IDENT) else {
        panic!("Missing '{BUTTON_IDENT}' field");
    };

    let TokenTree::Ident(button_ident) = button_ident_tt else {
        panic!("Expected button enum identifier, got: '{button_ident_tt}'");
    };

    let Some(buttons_tt) = map.get(BUTTONS) else {
        panic!("Missing '{BUTTONS}' field");
    };

    let TokenTree::Group(buttons_group) = buttons_tt else {
        panic!("Expected buttons array, got: '{buttons_tt}'");
    };

    let buttons = parse_buttons_array(buttons_group);

    let console_buttons = map.get(CONSOLE_BUTTONS).map_or(Vec::new(), |console_buttons_tt| {
        let TokenTree::Group(console_buttons_group) = console_buttons_tt else {
            panic!("Expected buttons array, got: '{console_buttons_tt}'");
        };
        parse_buttons_array(console_buttons_group)
    });

    (button_ident, buttons, console_buttons)
}

fn parse_buttons_array(group: &Group) -> Vec<Ident> {
    if group.delimiter() != Delimiter::Bracket {
        panic!("Expected '[' delimiter, got: '{group}'");
    }

    let mut buttons = Vec::new();

    let tokens_vec: Vec<_> = group.stream().into_iter().collect();
    let mut tokens = tokens_vec.as_slice();
    while !tokens.is_empty() {
        let TokenTree::Ident(ident) = &tokens[0] else {
            panic!("Expected identifier, got: '{}'", tokens[0]);
        };

        buttons.push(ident.clone());

        if tokens.len() > 1 && is_punct_match(&tokens[1], ',') {
            tokens = &tokens[2..];
        } else {
            tokens = &tokens[1..];
        }
    }

    buttons
}

fn parse_joypad_ident(map: &HashMap<String, TokenTree>) -> &Ident {
    let Some(joypad_ident) = map.get(JOYPAD_IDENT) else {
        panic!("Missing '{JOYPAD_IDENT}' field");
    };

    let TokenTree::Ident(joypad_ident) = joypad_ident else {
        panic!("Expected joypad struct identifier, got: '{joypad_ident}'");
    };

    joypad_ident
}

fn generate_button_enum(
    button_ident: &Ident,
    buttons: &[Ident],
    console_buttons: &[Ident],
) -> TokenStream {
    let all_buttons: Vec<_> = buttons.iter().chain(console_buttons).collect();

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
        pub enum #button_ident {
            #(#all_buttons,)*
        }
    }
}

fn generate_joypad_struct(
    button_enum_ident: &Ident,
    joypad_ident: &Ident,
    buttons: &[Ident],
) -> TokenStream {
    let struct_fields: Vec<_> = buttons
        .iter()
        .map(|button_ident| {
            let field_name = lowercase_ident(button_ident);
            quote! {
                pub #field_name: bool
            }
        })
        .collect();

    let struct_definition = quote! {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ::bincode::Encode, ::bincode::Decode)]
        pub struct #joypad_ident {
            #(#struct_fields,)*
        }
    };

    let mut match_arms: Vec<_> = buttons
        .iter()
        .map(|button_ident| {
            let field_name = lowercase_ident(button_ident);
            quote! {
                #button_enum_ident::#button_ident => self.#field_name = pressed
            }
        })
        .collect();
    match_arms.push(quote! {
        _ => {}
    });

    let struct_impl = quote! {
        impl #joypad_ident {
            #[inline]
            pub fn set_button(&mut self, button: #button_enum_ident, pressed: bool) {
                match button {
                    #(#match_arms,)*
                }
            }

            #[inline]
            #[must_use]
            pub fn with_button(mut self, button: #button_enum_ident, pressed: bool) -> Self {
                self.set_button(button, pressed);
                self
            }
        }
    };

    quote! {
        #struct_definition
        #struct_impl
    }
}

fn lowercase_ident(ident: &Ident) -> Ident {
    format_ident!("{}", ident.to_string().to_ascii_lowercase())
}

enum InputsStructField {
    Player(Ident),
    Button(Ident),
}

fn generate_inputs_struct(
    button_ident: &Ident,
    joypad_ident: &Ident,
    inputs_ident: &Ident,
    inputs_tt: &TokenTree,
) -> TokenStream {
    let TokenTree::Group(inputs_tt) = inputs_tt else {
        panic!("Expected inputs map, got: '{inputs_tt}'");
    };

    if inputs_tt.delimiter() != Delimiter::Brace {
        panic!("Expected '{{', got: '{inputs_tt}'");
    };

    let map = parse_map_tokens(&inputs_tt.stream().into_iter().collect::<Vec<_>>());

    let struct_fields: Vec<_> = map
        .into_iter()
        .map(|(field_name, tt)| {
            let TokenTree::Group(group) = tt else {
                panic!("Expected inputs field definition, got '{tt}'");
            };

            if group.delimiter() != Delimiter::Parenthesis {
                panic!("Expected '(', got '{group}'");
            }

            let group_tokens: Vec<_> = group.stream().into_iter().collect();
            if group_tokens.len() != 2 {
                panic!("Expected 2 tokens, got '{group}'");
            }

            let TokenTree::Ident(field_type) = &group_tokens[0] else {
                panic!("Expected inputs field identifier, got '{}'", group_tokens[0]);
            };

            let TokenTree::Ident(field_mapping) = &group_tokens[1] else {
                panic!("Expected inputs field mapping, got '{}'", group_tokens[1]);
            };

            let field = match field_type.to_string().as_str() {
                "Player" => InputsStructField::Player(field_mapping.clone()),
                "Button" => InputsStructField::Button(field_mapping.clone()),
                _ => panic!("Expected 'Player' or 'Button', got '{field_type}'"),
            };

            (format_ident!("{field_name}"), field)
        })
        .collect();

    let player_fields: Vec<_> = struct_fields
        .iter()
        .filter_map(|(field_name, field)| match field {
            InputsStructField::Player(ident) => Some((field_name, ident)),
            InputsStructField::Button(_) => None,
        })
        .collect();
    let button_fields: Vec<_> = struct_fields
        .iter()
        .filter_map(|(field_name, field)| match field {
            InputsStructField::Button(ident) => Some((field_name, ident)),
            InputsStructField::Player(_) => None,
        })
        .collect();

    let struct_player_fields: Vec<_> = player_fields
        .iter()
        .map(|(field_name, _)| {
            quote! {
                pub #field_name: #joypad_ident
            }
        })
        .collect();
    let struct_button_fields: Vec<_> = button_fields
        .iter()
        .map(|(field_name, _)| {
            quote! {
                pub #field_name: bool
            }
        })
        .collect();

    let struct_definition = quote! {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ::bincode::Encode, ::bincode::Decode)]
        pub struct #inputs_ident {
            #(#struct_player_fields,)*
            #(#struct_button_fields,)*
        }
    };

    let match_arms: Vec<_> = button_fields.iter().map(|(field_name, button)| {
        quote! {
            (#button_ident::#button, _) => self.#field_name = pressed
        }
    }).chain(
        player_fields.iter().map(|(field_name, player)| {
            quote! {
                (_, ::jgenesis_common::input::Player::#player) => self.#field_name.set_button(button, pressed)
            }
        })
    ).collect();

    let struct_impl = quote! {
        impl #inputs_ident {
            #[inline]
            pub fn set_button(&mut self, button: #button_ident, player: ::jgenesis_common::input::Player, pressed: bool) {
                match (button, player) {
                    #(#match_arms,)*
                }
            }

            #[inline]
            #[must_use]
            pub fn with_button(mut self, button: #button_ident, player: ::jgenesis_common::input::Player, pressed: bool) -> Self {
                self.set_button(button, player, pressed);
                self
            }
        }
    };

    quote! {
        #struct_definition
        #struct_impl
    }
}
