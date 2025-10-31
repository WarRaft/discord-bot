use convert_case::{Case, Casing};
use heck::ToShoutySnakeCase;
use proc_macro::TokenStream;
use quote::quote;
use syn::{Ident, ItemStruct, parse_macro_input};

pub fn expand(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as ItemStruct);
    let struct_name = &input.ident;

    let consts = input
        .fields
        .iter()
        .filter_map(|f| f.ident.as_ref())
        .map(|field| {
            let const_name = Ident::new(&field.to_string().to_shouty_snake_case(), field.span());
            let field_str = field.to_string().to_case(Case::Snake); // ← тут магия
            quote! {
                #[allow(dead_code)]
                pub const #const_name: &'static str = #field_str;
            }
        });

    let expanded = quote! {
        #input

        impl #struct_name {
            #(#consts)*
        }
    };

    TokenStream::from(expanded)
}
