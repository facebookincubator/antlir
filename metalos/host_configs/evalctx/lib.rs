/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#![deny(warnings)]
use starlark::environment::{Globals, GlobalsBuilder};
use thiserror::Error;

pub mod generator;
pub use generator::Generator;
pub use host;
#[cfg(feature = "facebook")]
pub use host::facebook;
pub use host::Host;
mod loader;
mod path;
mod template;

#[derive(Error, Debug)]
pub enum Error {
    /// The starlark module had syntax errors and could not be parsed.
    #[error("could not parse starlark: {0}")]
    Parse(anyhow::Error),
    /// The starlark module had valid syntax, but failed during evaluation.
    #[error("could not evaluate starlark module: {0}")]
    EvalModule(anyhow::Error),
    /// The starlark module had valid syntax, but contained an invalid load().
    #[error("'{src}' attempted to import non-existent module '{missing}'")]
    MissingImport {
        src: loader::ModuleId,
        missing: loader::ModuleId,
    },
    /// The generator was parsed and the module was evaluated, but it failed or
    /// returned the wrong type of output.
    #[error("could not evaluate generator function: {0}")]
    EvalGenerator(anyhow::Error),
    /// A starlark module was valid, but it didn't contain a 'generator'
    /// function.
    #[error("starlark module did not have generator function")]
    NotGenerator,
    /// Starlark errors that don't fit neatly into any user-caused categories.
    #[error("generic starlark error: {0}")]
    Starlark(anyhow::Error),
    /// The generator produced output, but it could not be applied to the
    /// system.
    #[error("could not apply generator output")]
    Apply(std::io::Error),
    /// Failed to load generator / other starlark source from the filesystem.
    #[error("could not load starlark source")]
    Load(std::io::Error),
    /// A template contained invalid syntax and could not be compiled.
    #[error("could not compile handlebars template")]
    TemplateCompile(#[from] handlebars::TemplateError),
    /// A template could not be rendered.
    #[error("could not render handlebars template")]
    TemplateRender(#[from] handlebars::RenderError),
    #[error("Failed to update shadow file with provided hashes: {0:?}")]
    PWHashError(anyhow::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

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
