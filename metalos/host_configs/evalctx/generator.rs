/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

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
use starlark::values::{list::ListOf, OwnedFrozenValue, Value, ValueLike};
use xattr::FileExt;

use crate::loader::{Loader, ModuleId};
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
    id: ModuleId,
    starlark_func: OwnedFrozenValue,
}

impl Generator {
    /// Recursively load a directory of starlark generators. These files must
    /// end in '.star' and define a function 'generator' that accepts a single
    /// ['metalos.Host'](crate::Host) parameter. Starlark files in the
    /// directory are available to be `load()`ed by generators.
    pub fn load(path: impl AsRef<Path>) -> Result<Vec<Self>> {
        Loader::load(path)?
            .into_iter()
            .map(|(id, module)| {
                let starlark_func = module.get("generator").ok_or(Error::NotGenerator)?;
                Ok(Self { id, starlark_func })
            })
            .filter(|r| match r {
                Err(Error::NotGenerator) => false,
                _ => true,
            })
            .collect()
    }

    pub fn id(&self) -> &ModuleId {
        &self.id
    }

    pub fn name(&self) -> String {
        self.id.to_string()
    }

    pub fn eval(self, host: &Host) -> Result<GeneratorOutput> {
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
    use tempfile::TempDir;

    // The hostname.star generator is super simple, so use that to test the
    // generator runtime implementation.
    #[test]
    fn hostname_generator() -> anyhow::Result<()> {
        let tmp_dir = TempDir::new()?;
        std::fs::write(
            tmp_dir.path().join("hostname.star"),
            include_str!("../generators/hostname.star"),
        )?;
        let mut generators = Generator::load(tmp_dir.path())?;
        assert_eq!(1, generators.len());
        let gen = generators.remove(0);
        let host = Host::example_host_for_tests();
        let output = gen.eval(&host)?;
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
        Ok(())
    }
}
