/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::ops::Deref;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use anyhow::Context;
use derive_more::Display;
use shadow::{ShadowFile, ShadowRecord};
use slog::{info, Logger};
use starlark::environment::{GlobalsBuilder, Module};
use starlark::eval::Evaluator;
use starlark::starlark_module;
use starlark::values::{dict::DictOf, list::ListOf, OwnedFrozenValue, Value, ValueLike};
use xattr::FileExt;

use crate::loader::{Loader, ModuleId};
use crate::path::PathExt;
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
    };
}

type Username = String;
type PWHash = String;

#[derive(Debug, Display, PartialEq, Eq, Clone)]
#[display(fmt = "{:?}", self)]
pub struct GeneratorOutput {
    pub files: Vec<File>,
    pub pw_hashes: Option<BTreeMap<Username, PWHash>>,
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
    fn file(path: &str, contents: &str, mode: Option<i32>) -> anyhow::Result<File> {
        Ok(File {
            path: path.into(),
            contents: contents.into(),
            mode: mode.map(|i| i as u32).unwrap_or(0o444),
        })
    }

    #[starlark(type("Dir"))]
    fn dir(path: &str, mode: Option<i32>) -> anyhow::Result<Dir> {
        Ok(Dir {
            path: path.into(),
            mode: mode.map(|i| i as u32).unwrap_or(0o555),
        })
    }

    #[starlark(type("GeneratorOutput"))]
    fn GeneratorOutput(
        files: Option<ListOf<Value>>,
        pw_hashes: Option<DictOf<Value, Value>>,
    ) -> anyhow::Result<GeneratorOutput> {
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
        Ok(GeneratorOutput { files, pw_hashes })
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

    pub fn eval(self, host: &HostIdentity) -> Result<GeneratorOutput> {
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
    pub fn apply(self, log: Logger, root: &Path) -> Result<()> {
        for file in self.files {
            let dst = root.force_join(file.path);
            info!(log, "Writing file: {:?}", dst);
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

        if let Some(pw_hashes) = self.pw_hashes {
            Self::apply_pw_hashes(log, pw_hashes, root).map_err(Error::PWHashError)?;
        }
        Ok(())
    }

    fn apply_pw_hashes(
        log: Logger,
        pw_hashes: BTreeMap<Username, PWHash>,
        root: &Path,
    ) -> anyhow::Result<()> {
        let shadow_file = root.join("etc/shadow");
        let mut shadow =
            ShadowFile::from_file(&shadow_file).context("Failed to load existing shadows file")?;

        for (user, hash) in pw_hashes.into_iter() {
            info!(log, "Updating hash for {} to {}", user, hash);
            let record =
                ShadowRecord::new(user, hash).context("failed to create shadow record for")?;
            shadow.update_record(record);
        }
        info!(
            log,
            "Shadow file {:?} internal data: {:?}", shadow_file, shadow
        );

        let content = shadow
            .write_to_file(&shadow_file)
            .context("failed to write mutated shadow file")?;
        info!(
            log,
            "Writing shadow file {:?} with content: {:?}", shadow_file, content
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{File, GeneratorOutput};
    use crate::{Generator, HostIdentity};
    use tempfile::TempDir;

    fn eval_one_generator(source: &'static str) -> anyhow::Result<GeneratorOutput> {
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
            eval_one_generator(include_str!("../generators/hostname.star"))?,
            GeneratorOutput {
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
def generator(host: metalos.HostIdentity) -> metalos.GeneratorOutput.type:
    return metalos.GeneratorOutput(
        files=[
            metalos.file(path="/test.json", contents=json({"a":"b","c":None})),
        ]
    )
        "#
            )?,
            GeneratorOutput {
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
