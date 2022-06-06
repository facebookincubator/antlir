/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

extern crate proc_macro;

use proc_macro::TokenStream;
use proc_macro_error::abort;
use quote::quote;
use syn::{parse_macro_input, DeriveInput};

/// Derive implementations for structs that are to be passed through to starlark
/// host config generators.
#[proc_macro_derive(StarlarkInput)]
pub fn derive_starlark_input(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    let name = input.ident;

    let expanded = quote! {
        // TODO(nga): this is normally done with `derive(ProvidesStaticType)`.
        //   Thrift types should better be wrapped in local struct to make it possible.
        unsafe impl starlark::values::ProvidesStaticType for #name {
            type StaticType = #name;
        }

        starlark::starlark_simple_value!(#name);
        impl<'v> starlark::values::StarlarkValue<'v> for #name {
            starlark::starlark_type!(stringify!(#name));
            starlark_derive::starlark_attrs!();
        }

        impl std::fmt::Display for #name {
            fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                write!(f, "{:#?}", self)
            }
        }
    };
    expanded.into()
}

/// Derive an implementation of Display that uses the pretty Debug format
#[proc_macro_derive(Display)]
pub fn derive_display(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = input.ident;
    let expanded = quote! {
        impl std::fmt::Display for #name {
            fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                write!(f, "{:#?}", self)
            }
        }
    };
    expanded.into()
}

fn test_attr(item: TokenStream, feature: &str) -> TokenStream {
    let f: syn::ItemFn = match syn::parse(item) {
        Ok(f) => f,
        Err(e) => abort!(e.span(), "test attributes may only be used on functions"),
    };
    let test_attr = match f.sig.asyncness.is_some() {
        true => quote!(tokio::test),
        false => quote!(std::prelude::v1::test),
    };
    let expanded = quote! {
        use tokio as _;
        #[cfg_attr(feature = #feature, #test_attr)]
        #[allow(dead_code)]
        #f
    };
    expanded.into()
}

/// Attribute for a regular unittest. Will be skipped during container and vm
/// test runs.
#[proc_macro_attribute]
pub fn test(_attr: TokenStream, item: TokenStream) -> TokenStream {
    test_attr(item, "metalos_plain_test")
}

/// Attribute for a container unittest. Will be skipped during regular and vm
/// test runs.
#[proc_macro_attribute]
pub fn containertest(_attr: TokenStream, item: TokenStream) -> TokenStream {
    test_attr(item, "metalos_container_test")
}

/// Attribute for a vm unittest. Will be skipped during regular and container
/// test runs.
#[proc_macro_attribute]
pub fn vmtest(_attr: TokenStream, item: TokenStream) -> TokenStream {
    test_attr(item, "metalos_vm_test")
}
