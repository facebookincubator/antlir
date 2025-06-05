/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use proc_macro_error::abort;
use proc_macro_error::proc_macro_error;
use quote::ToTokens;
use quote::format_ident;
use quote::quote;
use syn::ItemImpl;
use syn::parse_macro_input;

/// A proc-macro that ensures consistency in Fact impls.
///
/// It wraps #[typetag::serde] with the correct name and automatically
/// implements the FactKind trait.
#[proc_macro_error]
#[proc_macro_attribute]
pub fn fact_impl(
    attr: proc_macro::TokenStream,
    item: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    let attr = proc_macro2::TokenStream::from(attr);

    // We use the fully qualified type name as the typetag name since downstream
    // consumer queries have been written to expect that.
    // At this time, it is not really possible to infer the type name as a
    // literal string within this proc-macro, so we require the author to pass
    // it in as a string literal, and generate a test to make sure it is correct.
    let qualname = match syn::parse2::<syn::LitStr>(attr.clone()) {
        Ok(lit) => lit,
        Err(_) => abort!(
            attr,
            "Expected a string literal of the fully-qualified type name"
        ),
    };

    let input = parse_macro_input!(item as ItemImpl);

    let self_ty = &input.self_ty;

    let generated_test_fn = format_ident!(
        "test_antlir2_facts_typetag_{}",
        self_ty
            .to_token_stream()
            .to_string()
            .replace(" ", "_")
            .replace("::", "_")
    );

    // Generate the output with typetag::serde attribute
    let output = quote! {
        impl ::antlir2_facts::fact::FactKind for #self_ty {
            const KIND: &'static str = #qualname;
        }

        #[cfg(test)]
        #[test]
        fn #generated_test_fn() {
            assert_eq!(#qualname, std::any::type_name::<#self_ty>());
        }

        use ::antlir2_facts::__private::typetag::*;
        #[typetag::serde(name = #qualname)]
        #input
    };

    output.into()
}
