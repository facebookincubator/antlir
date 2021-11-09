/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::ffi::OsStr;
use std::fs;
use std::io::Write;
use std::ops::Deref;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use anyhow::Context;
use derive_more::Display;
use starlark::environment::{GlobalsBuilder, Module};
use starlark::eval::Evaluator;
use starlark::starlark_module;
use starlark::syntax::{AstModule, Dialect};
use starlark::values::{list::ListOf, OwnedFrozenValue, Value, ValueLike};
use xattr::FileExt;

use crate::path::PathExt;
use crate::{Error, Host, Result};

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

#[derive(Debug, Display, PartialEq, Eq, Clone)]
#[display(fmt = "{:?}", self)]
pub struct GeneratorOutput {
    pub files: Vec<File>,
}
output_only_struct!(GeneratorOutput);

#[derive(Debug, PartialEq, Eq, Clone, Display)]
#[display(fmt = "{:?}", self)]
pub struct Dir {
    path: PathBuf,
    mode: u32,
}
output_only_struct!(Dir);

#[derive(Display, PartialEq, Eq, Clone)]
#[display(fmt = "{:?}", self)]
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
    #[starlark(type("File"))]
    fn file(path: &str, contents: &str, mode: Option<i32>) -> File {
        Ok(File {
            path: path.into(),
            contents: contents.into(),
            mode: mode.map(|i| i as u32).unwrap_or(0o444),
        })
    }

    #[starlark(type("Dir"))]
    fn dir(path: &str, mode: Option<i32>) -> Dir {
        Ok(Dir {
            path: path.into(),
            mode: mode.map(|i| i as u32).unwrap_or(0o555),
        })
    }

    #[starlark(type("GeneratorOutput"))]
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
                .collect::<anyhow::Result<_>>()?,
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
            AstModule::parse(name.as_ref(), src.as_ref().to_owned(), &Dialect::Extended)
                .map_err(Error::Parse)?;
        let module = Module::new();
        let globals = crate::globals();
        let mut evaluator: Evaluator = Evaluator::new(&module);
        let name = name.as_ref().to_owned();

        evaluator
            .eval_module(ast, &globals)
            .map_err(Error::EvalModule)?;

        let module = module.freeze().map_err(Error::Starlark)?;
        let starlark_func = module.get("generator").ok_or(Error::NotGenerator)?;
        Ok(Self {
            name,
            starlark_func,
        })
    }

    /// Recursively load a directory of .star generators or individual file paths
    pub fn load(path: &Path) -> Result<Vec<Self>> {
        match std::fs::metadata(path).map_err(Error::Load)?.is_dir() {
            true => Ok(std::fs::read_dir(path)
                .map_err(Error::Load)?
                .into_iter()
                .filter_map(std::io::Result::ok)
                .filter(|entry| entry.file_type().map(|t| !t.is_symlink()).unwrap_or(false))
                .map(|entry| Self::load(&entry.path()))
                .collect::<Result<Vec<_>>>()?
                .into_iter()
                .flatten()
                .collect()),
            false => match path.extension() == Some(OsStr::new("star")) {
                true => {
                    let src = std::fs::read_to_string(path).map_err(Error::Load)?;
                    Self::compile(path.display().to_string(), src).map(|gen| vec![gen])
                }
                false => Ok(vec![]),
            },
        }
    }

    pub fn eval(&self, host: &Host) -> Result<GeneratorOutput> {
        let module = Module::new();
        let mut evaluator = Evaluator::new(&module);
        let host_value = evaluator.heap().alloc(host.clone());
        let output = evaluator
            .eval_function(self.starlark_func.value(), &[], &[("host", host_value)])
            .map_err(Error::EvalGenerator)?;
        // clone the result off the heap so that the Evaluator and Module can be safely dropped
        Ok(GeneratorOutput::from_value(output)
            .context("expected 'generator' to return 'metalos.GeneratorOutput'")
            .map_err(Error::EvalGenerator)?
            .deref()
            .clone())
    }
}

impl GeneratorOutput {
    pub fn apply(self, root: impl PathExt) -> Result<()> {
        for file in self.files {
            let dst = root.force_join(file.path);
            let mut f = fs::File::create(&dst).map_err(Error::Apply)?;
            f.write_all(&file.contents).map_err(Error::Apply)?;
            let mut perms = f.metadata().map_err(Error::Apply)?.permissions();
            perms.set_mode(file.mode);
            f.set_permissions(perms).map_err(Error::Apply)?;
            // Try to mark the file as metalos-generated, but swallow the error
            // if we can't. It's ok to fail silently here, because the only use
            // case for this xattr is debugging tools, and it's better to have
            // debug tools miss some files that come from generators, rather
            // than fail to apply configs entirely
            let _ = f.set_xattr("user.metalos.generator", &[1]);
        }
        Ok(())
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
        let host = Host::example_host_for_tests();
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
