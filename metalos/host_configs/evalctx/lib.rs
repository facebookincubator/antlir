/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#![deny(warnings)]
use starlark::environment::{Globals, GlobalsBuilder};

pub mod generator;
pub use generator::Generator;
pub use host;
#[cfg(feature = "facebook")]
pub use host::facebook;
pub use host::Host;
mod path;
mod template;

pub fn metalos(builder: &mut GlobalsBuilder) {
    builder.struct_("metalos", |builder: &mut GlobalsBuilder| {
        generator::module(builder);
        template::module(builder);
    });
}

pub fn globals() -> Globals {
    GlobalsBuilder::extended().with(metalos).build()
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
