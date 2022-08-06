/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#![deny(warnings)]
use thiserror::Error;

pub mod generator;
mod path;
mod starlark;
pub use generator::Generator;

pub use crate::starlark::generator::StarlarkGenerator;

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
        src: crate::starlark::loader::ModuleId,
        missing: crate::starlark::loader::ModuleId,
    },
    /// The starlark module did not have a parent or was not a file name
    #[error("Pathbuf did not pass invariant checks for ModuleId: {0}")]
    CreateModule(anyhow::Error),
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
    #[error("could not apply generator output: {0}")]
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
    #[error("while serializing thrift struct for starlark: {0:?}")]
    PrepStarlarkStruct(anyhow::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
