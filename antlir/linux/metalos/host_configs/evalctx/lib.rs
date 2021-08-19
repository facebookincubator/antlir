/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#![deny(warnings)]
use starlark::environment::{Globals, GlobalsBuilder};

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
mod template;

pub fn metalos(builder: &mut GlobalsBuilder) {
    builder.struct_("metalos", |builder: &mut GlobalsBuilder| {
        template::module(builder);
    });
}

pub fn globals() -> Globals {
    GlobalsBuilder::new().with(metalos).build()
}

#[cfg(test)]
mod tests {
    use super::metalos;
    use starlark::assert::Assert;
    #[test]
    fn starlark_module_exposed() {
        let mut a = Assert::new();
        a.globals_add(metalos);
        a.pass("metalos.template(\"\")");
    }
}
