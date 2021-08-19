/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#![deny(warnings)]

/// Helper macro for a simple struct that is just exposing fields to Starlark.
/// It handles the boilerplate of Starlark impls that are required for this
/// basic case.
macro_rules! simple_data_struct {
    ($x:ident) => {
        starlark::starlark_simple_value!($x);
        impl<'v> starlark::values::StarlarkValue<'v> for $x {
            starlark::starlark_type!(stringify!($x));
            starlark_module::starlark_attrs!();
        }

        paste::paste! {
            impl $x {
                #[allow(dead_code)]
                pub fn builder() -> [< $x Builder >] {
                    [< $x Builder >]::default()
                }
            }
        }
    };
}

mod host;
