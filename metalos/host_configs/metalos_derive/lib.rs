/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

extern crate proc_macro;

use quote::quote;
use syn::{parse_macro_input, DeriveInput};

#[proc_macro_derive(StarlarkInput)]
pub fn derive_starlark_input(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    let name = input.ident;

    let expanded = quote! {
        starlark::starlark_simple_value!(#name);
        impl<'v> starlark::values::StarlarkValue<'v> for #name {
            starlark::starlark_type!(stringify!(#name));
            starlark_derive::starlark_attrs!();
        }

        impl std::fmt::Display for #name {
            fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                write!(f, "{:?}", self)
            }
        }
    };
    expanded.into()
}

#[proc_macro_derive(Display)]
pub fn derive_display(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = input.ident;
    let expanded = quote! {
        impl std::fmt::Display for #name {
            fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                write!(f, "{:?}", self)
            }
        }
    };
    expanded.into()
}
