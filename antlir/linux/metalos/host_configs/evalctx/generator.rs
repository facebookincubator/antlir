/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::ops::Deref;
use std::path::PathBuf;

use anyhow::{Context, Result};
use starlark::environment::{GlobalsBuilder, Module};
use starlark::eval::Evaluator;
use starlark::starlark_module;
use starlark::syntax::{AstModule, Dialect};
use starlark::values::{list::ListOf, OwnedFrozenValue, Value};

use crate::Host;

// Macro-away all the Starlark boilerplate for structs that are _only_ returned
// from Starlark, and are not expected to be able to be read/used from the
// generator Starlark code.
macro_rules! output_only_struct {
    ($x:ident) => {
        starlark::starlark_simple_value!($x);
        impl<'v> starlark::values::StarlarkValue<'v> for $x {
            starlark::starlark_type!(stringify!($x));
        }
    };
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct GeneratorOutput {
    pub files: Vec<File>,
}
output_only_struct!(GeneratorOutput);

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Dir {
    path: PathBuf,
    mode: u32,
}
output_only_struct!(Dir);

#[derive(PartialEq, Eq, Clone)]
pub struct File {
    pub path: PathBuf,
    pub contents: Vec<u8>,
    pub mode: u32,
}
output_only_struct!(File);

impl std::fmt::Debug for File {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("File")
            .field("path", &self.path)
            .field("mode", &format!("{:#o}", &self.mode))
            .field(
                "contents",
                &std::str::from_utf8(&self.contents).unwrap_or("<binary data>"),
            )
            .finish()
    }
}

#[starlark_module]
pub fn module(registry: &mut GlobalsBuilder) {
    // TODO: accept symbolic strings in 'mode' as well
    #[starlark_type("File")]
    fn file(path: &str, contents: &str, mode: Option<i32>) -> File {
        Ok(File {
            path: path.into(),
            contents: contents.into(),
            mode: mode.map(|i| i as u32).unwrap_or(0o444),
        })
    }

    #[starlark_type("Dir")]
    fn dir(path: &str, mode: Option<i32>) -> Dir {
        Ok(Dir {
            path: path.into(),
            mode: mode.map(|i| i as u32).unwrap_or(0o555),
        })
    }

    #[starlark_type("GeneratorOutput")]
    fn GeneratorOutput(files: Option<ListOf<Value>>) -> GeneratorOutput {
        let files: Vec<File> = match files {
            Some(files) => files
                .to_vec()
                .into_iter()
                .map(|v| {
                    v.downcast_ref::<File>()
                        .with_context(|| format!("{:?} is not a File", v))
                        .map(|f| f.deref().clone())
                })
                .collect::<Result<_>>()?,
            None => vec![],
        };
        Ok(GeneratorOutput { files })
    }

    // this must match the type name returned by the Host struct
    const Host: &str = "Host";
}

pub struct Generator {
    pub name: String,
    starlark_func: OwnedFrozenValue,
}

impl Generator {
    pub fn compile<'a, N: AsRef<str>, S: AsRef<str>>(name: N, src: S) -> Result<Self> {
        let ast: AstModule =
            AstModule::parse(name.as_ref(), src.as_ref().to_owned(), &Dialect::Extended)?;
        let module = Module::new();
        let globals = crate::globals();
        let mut evaluator: Evaluator = Evaluator::new(&module, &globals);
        let name = name.as_ref().to_owned();

        evaluator
            .eval_module(ast)
            .with_context(|| format!("failed to evaluate starlark module '{}'", &name))?;

        let module = module.freeze()?;
        let starlark_func = module
            .get("generator")
            .with_context(|| format!("module '{}' must have 'generator' function", &name))?;
        Ok(Self {
            name,
            starlark_func,
        })
    }

    pub fn eval(&self, host: &Host) -> Result<GeneratorOutput> {
        let module = Module::new();
        let globals = crate::globals();
        let mut evaluator = Evaluator::new(&module, &globals);
        let host_value = evaluator.heap().alloc(host.clone());
        let output =
            evaluator.eval_function(self.starlark_func.value(), &[], &[("host", host_value)])?;
        // clone the result off the heap so that the Evaluator and Module can be safely dropped
        Ok(GeneratorOutput::from_value(output)
            .context("expected 'generator' to return 'metalos.GeneratorOutput'")?
            .deref()
            .clone())
    }
}

#[cfg(test)]
mod tests {
    use super::{File, GeneratorOutput};
    use crate::{Generator, Host};

    // The hostname.star generator is super simple, so use that to test the
    // generator runtime implementation.
    #[test]
    fn hostname_generator() {
        let gen = Generator::compile("hostname.star", include_str!("../generators/hostname.star"))
            .unwrap();
        let host = Host::default();
        let output = gen.eval(&host).unwrap();
        assert_eq!(
            output,
            GeneratorOutput {
                files: vec![File {
                    path: "/etc/hostname".into(),
                    contents: "host001.01.abc0.facebook.com\n".into(),
                    mode: 0o444,
                }]
            }
        );
    }
}
