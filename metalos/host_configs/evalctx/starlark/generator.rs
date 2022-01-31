/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeMap;
use std::fmt::Display;
use std::ops::Deref;
use std::path::Path;

use anyhow::Context;
use starlark::environment::{GlobalsBuilder, Module};
use starlark::eval::Evaluator;
use starlark::starlark_module;
use starlark::values::{dict::DictOf, list::ListOf, OwnedFrozenValue, Value, ValueLike};

use crate::generator::{Dir, File, Output};
use crate::starlark::loader::{Loader, ModuleId};
use crate::{Error, HostIdentity, Result};

// Macro-away all the Starlark boilerplate for structs that are _only_ returned
// from Starlark, and are not expected to be able to be read/used from the
// generator Starlark code.
macro_rules! output_only_struct {
    ($x:ident) => {
        starlark::starlark_simple_value!($x);
        impl<'v> starlark::values::StarlarkValue<'v> for $x {
            starlark::starlark_type!(stringify!($x));
        }

        impl Display for $x {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{:#?}", self)
            }
        }
    };
}

type Username = String;
type PWHash = String;

output_only_struct!(Output);
output_only_struct!(Dir);
output_only_struct!(File);

#[starlark_module]
pub fn module(registry: &mut GlobalsBuilder) {
    // TODO: accept symbolic strings in 'mode' as well
    #[starlark(type("File"))]
    fn file(path: &str, contents: &str, mode: Option<i32>) -> anyhow::Result<File> {
        Ok(File {
            path: path.into(),
            contents: contents.into(),
            mode: mode.map_or(0o444, |i| i as u32),
        })
    }

    #[starlark(type("Dir"))]
    fn dir(path: &str, mode: Option<i32>) -> anyhow::Result<Dir> {
        Ok(Dir {
            path: path.into(),
            mode: mode.map_or(0o555, |i| i as u32),
        })
    }

    #[starlark(type("Output"))]
    fn Output(
        files: Option<ListOf<Value>>,
        pw_hashes: Option<DictOf<Value, Value>>,
    ) -> anyhow::Result<Output> {
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
        let pw_hashes: Option<BTreeMap<Username, PWHash>> = match pw_hashes {
            Some(hashes) => Some(
                hashes
                    .collect_entries()
                    .into_iter()
                    .map(|(k, v)| {
                        Ok((
                            k.unpack_str()
                                .context(format!("provided key {:?} was not a string", k))?
                                .to_string(),
                            v.unpack_str()
                                .context(format!("provided value {:?} was not a string", v))?
                                .to_string(),
                        ))
                    })
                    .collect::<anyhow::Result<_>>()
                    .context("Failed to convert PW hashes from starlark to BTreeMap")?,
            ),
            None => None,
        };
        Ok(Output { files, pw_hashes })
    }

    // this must match the type name returned by the HostIdentity struct
    const HostIdentity: &str = "HostIdentity";
}

pub struct Generator {
    id: ModuleId,
    starlark_func: OwnedFrozenValue,
}

impl Generator {
    /// Recursively load a directory of starlark generators. These files must
    /// end in '.star' and define a function 'generator' that accepts a single
    /// [metalos.HostIdentity](crate::HostIdentity) parameter. Starlark files in
    /// the directory are available to be `load()`ed by generators.
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

    pub fn eval(self, host: &HostIdentity) -> Result<Output> {
        let module = Module::new();
        let mut evaluator = Evaluator::new(&module);
        let host_value = evaluator.heap().alloc(host.clone());
        let output = evaluator
            .eval_function(self.starlark_func.value(), &[], &[("host", host_value)])
            .map_err(Error::EvalGenerator)?;
        // clone the result off the heap so that the Evaluator and Module can be safely dropped
        Ok(Output::from_value(output)
            .context("expected 'generator' to return 'metalos.Output'")
            .map_err(Error::EvalGenerator)?
            .deref()
            .clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn eval_one_generator(source: &'static str) -> anyhow::Result<Output> {
        let tmp_dir = TempDir::new()?;
        std::fs::write(tmp_dir.path().join("test_generator.star"), source)?;
        let mut generators = Generator::load(tmp_dir.path())?;
        assert_eq!(1, generators.len());
        let gen = generators.remove(0);
        let host = HostIdentity::example_host_for_tests();
        let result = gen.eval(&host)?;
        Ok(result)
    }

    // The hostname.star generator is super simple, so use that to test the
    // generator runtime implementation.
    #[test]
    fn hostname_generator() -> anyhow::Result<()> {
        assert_eq!(
            eval_one_generator(include_str!("../../generators/hostname.star"))?,
            Output {
                files: vec![File {
                    path: "/etc/hostname".into(),
                    contents: "host001.01.abc0.facebook.com\n".into(),
                    mode: 0o444,
                }],
                pw_hashes: None,
            }
        );
        Ok(())
    }

    // We use "extended" version of the Starlark language which includes
    // a built-in function that generates JSON, among other things. We may
    // rely on the presence and behaviour of this function when generating
    // some configs
    #[test]
    fn generator_with_json_call() -> anyhow::Result<()> {
        assert_eq!(
            eval_one_generator(
                r#"
def generator(host: metalos.HostIdentity) -> metalos.Output.type:
    return metalos.Output(
        files=[
            metalos.file(path="/test.json", contents=json({"a":"b","c":None})),
        ]
    )
        "#
            )?,
            Output {
                files: vec![File {
                    path: "/test.json".into(),
                    contents: r#"{"a": "b", "c": null}"#.into(),
                    mode: 0o444,
                }],
                pw_hashes: None,
            }
        );
        Ok(())
    }
}
