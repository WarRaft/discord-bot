extern crate proc_macro;

use proc_macro::TokenStream;

mod define_field_names;

#[proc_macro_attribute]
pub fn define_field_names(attr: TokenStream, item: TokenStream) -> TokenStream {
    define_field_names::expand(attr, item)
}
