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
use starlark::values::dict::DictOf;
use starlark::values::list::ListOf;
use starlark::values::{OwnedFrozenValue, StarlarkValue, Value, ValueLike};

use crate::generator::{Dir, File, Generator, Output};
use crate::starlark::loader::{Loader, ModuleId};
use crate::{Error, Result};
use metalos_host_configs::provisioning_config::ProvisioningConfig;
use starlark_util::Struct;

// Macro-away all the Starlark boilerplate for structs that are _only_ returned
// from Starlark, and are not expected to be able to be read/used from the
// generator Starlark code.
macro_rules! output_only_struct {
    ($x:ident) => {
        // TODO(nga): this is normally done with `derive(ProvidesStaticType)`.
        //   Thrift types should better be wrapped in local struct to make it possible.
        unsafe impl starlark::values::ProvidesStaticType for $x {
            type StaticType = $x;
        }

        starlark::starlark_simple_value!($x);
        impl<'v> starlark::values::StarlarkValue<'v> for $x {
            starlark::starlark_type!(stringify!($x));
        }

        impl serde::Serialize for $x {
            fn serialize<S>(&self, _s: S) -> std::result::Result<S::Ok, S::Error>
            where
                S: serde::Serializer,
            {
                Err(serde::ser::Error::custom(format!(
                    "{} isn't serializable",
                    stringify!($x)
                )))
            }
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

fn collect_list_of<'v, T>(lst: ListOf<'v, Value<'v>>) -> anyhow::Result<Vec<T>>
where
    T: StarlarkValue<'v> + Clone,
{
    lst.to_vec()
        .into_iter()
        .map(|v| {
            let owned: T = v
                .downcast_ref()
                .with_context(|| format!("{:?} is not a {}", v, std::any::type_name::<T>()))
                .map(|v: &T| v.clone())?;
            Ok(owned)
        })
        .collect()
}

#[starlark_module]
pub fn module(registry: &mut GlobalsBuilder) {
    // TODO: accept symbolic strings in 'mode' as well
    #[starlark(type = "File")]
    fn file(path: &str, contents: &str, mode: Option<i32>) -> anyhow::Result<File> {
        Ok(File {
            path: path.into(),
            contents: contents.into(),
            mode: mode.map_or(0o444, |i| i as u32),
        })
    }

    #[starlark(type = "Dir")]
    fn dir(path: &str) -> anyhow::Result<Dir> {
        Ok(Dir { path: path.into() })
    }

    #[starlark(type = "Output")]
    fn Output<'v>(
        files: Option<ListOf<'v, Value<'v>>>,
        dirs: Option<ListOf<'v, Value<'v>>>,
        pw_hashes: Option<DictOf<'v, Value<'v>, Value<'v>>>,
    ) -> anyhow::Result<Output> {
        let files = files.map_or_else(|| Ok(vec![]), collect_list_of)?;
        let dirs = dirs.map_or_else(|| Ok(vec![]), collect_list_of)?;
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
        Ok(Output {
            files,
            dirs,
            pw_hashes,
        })
    }

    const ProvisioningConfig: &str = std::any::type_name::<ProvisioningConfig>();
}

pub struct StarlarkGenerator {
    id: ModuleId,
    starlark_func: OwnedFrozenValue,
}

impl StarlarkGenerator {
    /// Recursively load a directory of starlark generators. These files must
    /// end in '.star' and define a function 'generator' that accepts a single
    /// [metalos.ProvisioningConfig](crate::ProvisioningConfig) parameter.
    /// Starlark files in the directory are available to be `load()`ed by
    /// generators.
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
}

impl Generator for StarlarkGenerator {
    fn name(&self) -> &str {
        self.id.as_str()
    }

    fn eval(&self, prov: &ProvisioningConfig) -> Result<Output> {
        let module = Module::new();
        let mut evaluator = Evaluator::new(&module);
        let provisioning_config = evaluator
            .heap()
            .alloc(Struct::new(prov).map_err(Error::PrepStarlarkStruct)?);
        let output = evaluator
            .eval_function(
                self.starlark_func.value(),
                &[],
                &[("prov", provisioning_config)],
            )
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
    use starlark::codemap::FileSpanRef;
    use starlark::environment::Module;
    use starlark::eval::Evaluator;
    use starlark::syntax::{AstModule, Dialect};
    use std::cell::{RefCell, RefMut};
    use std::collections::BTreeSet;
    use std::env;
    use std::ffi::OsStr;
    use std::path::Path;
    use std::rc::Rc;
    use tempfile::TempDir;
    use walkdir::WalkDir;

    fn eval_one_generator(source: &'static str) -> anyhow::Result<Output> {
        let tmp_dir = TempDir::new()?;
        std::fs::write(tmp_dir.path().join("test_generator.star"), source)?;
        let mut generators = StarlarkGenerator::load(tmp_dir.path())?;
        assert_eq!(1, generators.len());
        let gen = generators.remove(0);
        let host = example_host_for_tests::example_host_for_tests();
        let result = gen.eval(&host.provisioning_config)?;
        Ok(result)
    }

    // The hostname.star generator is super simple, so use that to test the
    // generator runtime implementation.
    #[test]
    fn hostname_generator() -> anyhow::Result<()> {
        assert_eq!(
            eval_one_generator(
                r#"
def generator(prov: metalos.ProvisioningConfig) -> metalos.Output.type:
    return metalos.Output(
        files=[
            metalos.file(path="/etc/hostname", contents=prov.identity.hostname + "\n"),
        ]
    )
            "#
            )?,
            Output {
                files: vec![File {
                    path: "/etc/hostname".into(),
                    contents: "host001.01.abc0.facebook.com\n".into(),
                    mode: 0o444,
                }],
                dirs: vec![],
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
def generator(prov: metalos.ProvisioningConfig) -> metalos.Output.type:
    return metalos.Output(
        files=[
            metalos.file(path="/test.json", contents=json.encode({"a":"b","c":None})),
        ]
    )
        "#
            )?,
            Output {
                files: vec![File {
                    path: "/test.json".into(),
                    contents: r#"{"a":"b","c":null}"#.into(),
                    mode: 0o444,
                }],
                dirs: vec![],
                pw_hashes: None,
            }
        );
        Ok(())
    }

    #[test]
    fn generator_with_dir() -> anyhow::Result<()> {
        assert_eq!(
            eval_one_generator(
                r#"
def generator(prov: metalos.ProvisioningConfig) -> metalos.Output.type:
    return metalos.Output(
        dirs=[
            metalos.dir(path="/dir"),
        ]
    )
        "#
            )?,
            Output {
                files: vec![],
                dirs: vec![Dir {
                    path: "/dir".into(),
                }],
                pw_hashes: None,
            }
        );
        Ok(())
    }

    fn eval_one_generator_coverage(file_path: &Path) -> anyhow::Result<()> {
        // we need to put the test in a temp dir because `Generator::load` understands
        // only directories.
        let tmp_dir = TempDir::new()?;
        let filename = file_path
            .file_name()
            .unwrap_or_else(|| OsStr::new("test.star"));
        std::fs::copy(file_path, tmp_dir.path().join(filename))?;

        // get total number of statements and the lines numebrs that are supposed to be executed,
        // they will be used to calculate coverage.
        let src_code = std::fs::read_to_string(file_path)?;
        let ast =
            AstModule::parse(&filename.to_string_lossy(), src_code, &Dialect::Extended).unwrap();
        let total_num_statements = ast.stmt_locations().len();
        assert_ne!(0, total_num_statements);
        let to_visit_lines: BTreeSet<u16> = ast
            .stmt_locations()
            .into_iter()
            .map(|line| line.resolve_span().begin_line as u16)
            .collect();

        let visited_lines: Rc<RefCell<_>> = Rc::new(RefCell::new(BTreeSet::new()));
        let before_stmt = |span: FileSpanRef, _eval: &mut Evaluator<'_, '_>| {
            let mut set: RefMut<_> = visited_lines.borrow_mut();
            set.insert(span.resolve_span().begin_line as u16);
        };

        let module = Module::new();
        let mut evaluator = Evaluator::new(&module);
        evaluator.before_stmt(&before_stmt);
        let globals = crate::starlark::globals();
        evaluator.eval_module(ast, &globals)?;

        let host = example_host_for_tests::example_host_for_tests();
        let prov_value = evaluator
            .heap()
            .alloc(Struct::new(&host.provisioning_config).map_err(Error::PrepStarlarkStruct)?);

        match module.get("generator") {
            None => anyhow::bail!(
                "Starlark file {:?} does not have a generator function",
                file_path.file_name()
            ),
            Some(function) => {
                evaluator.eval_function(function, &[], &[("prov", prov_value)])?;
                assert_eq!(
                    to_visit_lines,
                    visited_lines.borrow().clone(),
                    "Starlark file {:?} has branches that are not executed",
                    file_path.file_name()
                );
            }
        }
        Ok(())
    }

    #[test]
    fn test_all_generators_coverage() -> anyhow::Result<()> {
        // find all *.star files and test them to make sure:
        // * they run successufully
        // * their coverage is 100% by using starlark::eval::Evaluator::before_stmt
        let test_file_dir = env::var("TEST_FILES_DIR")?;
        for entry in WalkDir::new(test_file_dir)
            .follow_links(true)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
            .filter(|e| e.path().extension() == Some(OsStr::new("star")))
        {
            eval_one_generator_coverage(entry.path())?;
        }
        Ok(())
    }
}
