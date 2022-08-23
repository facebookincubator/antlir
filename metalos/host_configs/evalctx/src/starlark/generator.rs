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

use anyhow::bail;
use anyhow::Context;
use metalos_host_configs::provisioning_config::ProvisioningConfig;
use starlark::environment::GlobalsBuilder;
use starlark::environment::Module;
use starlark::eval::Evaluator;
use starlark::starlark_module;
use starlark::values::dict::DictOf;
use starlark::values::list::ListOf;
use starlark::values::OwnedFrozenValue;
use starlark::values::StarlarkValue;
use starlark::values::Value;
use starlark::values::ValueLike;
use starlark_util::Struct;

use crate::generator::Dir;
use crate::generator::File;
use crate::generator::Generator;
use crate::generator::Output;
use crate::generator::ZeroFile;
use crate::starlark::loader::Loader;
use crate::starlark::loader::ModuleId;
use crate::Error;
use crate::Result;

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
output_only_struct!(ZeroFile);

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

    #[starlark(type = "ZeroFile")]
    fn zero_file(
        path: &str,
        block_size_bytes: i32,
        block_count: i32,
        mode: Option<i32>,
    ) -> anyhow::Result<ZeroFile> {
        if block_size_bytes <= 0 {
            bail!("Block size {block_size_bytes} must be positive");
        }
        if block_count <= 0 {
            bail!("Block count {block_count} must be positive");
        }
        Ok(ZeroFile {
            path: path.into(),
            // note: we assume usize can hold an i32 without truncation
            block_size_bytes: block_size_bytes as usize,
            block_count: block_count as u32,
            mode: mode.map_or(0o444, |i| i as u32),
        })
    }

    #[starlark(type = "Output")]
    fn Output<'v>(
        files: Option<ListOf<'v, Value<'v>>>,
        dirs: Option<ListOf<'v, Value<'v>>>,
        pw_hashes: Option<DictOf<'v, Value<'v>, Value<'v>>>,
        zero_files: Option<ListOf<'v, Value<'v>>>,
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
        let zero_files = zero_files.map_or_else(|| Ok(vec![]), collect_list_of)?;
        Ok(Output {
            files,
            dirs,
            pw_hashes,
            zero_files,
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
                let starlark_func = module.get("generator").map_err(|_| Error::NotGenerator)?;
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
    use std::collections::BTreeSet;
    use std::env;
    use std::ffi::OsStr;
    use std::path::Path;

    use starlark::environment::Module;
    use starlark::eval::Evaluator;
    use starlark::eval::ProfileMode;
    use starlark::syntax::AstModule;
    use starlark::syntax::Dialect;
    use tempfile::TempDir;
    use walkdir::WalkDir;

    use super::*;

    fn eval_one_generator(source: &str) -> anyhow::Result<Output> {
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
                zero_files: vec![],
            }
        );
        Ok(())
    }

    // Exercise the ZeroFile path
    #[test]
    fn zerofile_generator() -> anyhow::Result<()> {
        assert_eq!(
            eval_one_generator(
                r#"
def generator(prov: metalos.ProvisioningConfig) -> metalos.Output.type:
    return metalos.Output(
        zero_files=[
            metalos.zero_file(path="/swapvol/swapfile", block_size_bytes=4096, block_count=10),
        ]
    )
            "#
            )?,
            Output {
                files: vec![],
                dirs: vec![],
                pw_hashes: None,
                zero_files: vec![ZeroFile {
                    path: "/swapvol/swapfile".into(),
                    block_size_bytes: 4096,
                    block_count: 10,
                    mode: 0o444,
                }],
            }
        );
        Ok(())
    }

    // Blocksize must be positive
    #[test]
    fn zerofile_bad_blocksize() -> anyhow::Result<()> {
        assert!(
            eval_one_generator(
                r#"
    def generator(prov: metalos.ProvisioningConfig) -> metalos.Output.type:
        return metalos.Output(
            files=[
                metalos.zero_file(path="/swapvol/swapfile", block_size_bytes=0, block_count=10),
            ]
        )
                "#
            )
            .is_err()
        );
        Ok(())
    }

    // Blocksize must be positive
    #[test]
    fn zerofile_bad_blockcount() -> anyhow::Result<()> {
        assert!(
            eval_one_generator(
                r#"
    def generator(prov: metalos.ProvisioningConfig) -> metalos.Output.type:
        return metalos.Output(
            files=[
                metalos.zero_file(path="/swapvol/swapfile", block_size_bytes=4096, block_count=0),
            ]
        )
                "#
            )
            .is_err()
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
                zero_files: vec![],
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
                zero_files: vec![],
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

        let module = Module::new();
        let mut evaluator = Evaluator::new(&module);
        evaluator
            .enable_profile(&ProfileMode::Statement)
            .expect("Unable to enable profile");
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
                let visited_lines: BTreeSet<_> = evaluator
                    .coverage()?
                    .into_iter()
                    .map(|s| s.span.begin_line as u16)
                    .collect();
                assert_eq!(
                    to_visit_lines,
                    visited_lines,
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

    #[test]
    fn test_systemd_networkd_correctness() -> anyhow::Result<()> {
        let test_file_dir = env::var("TEST_FILES_DIR")?;
        let file_path = test_file_dir + "/" + "systemd-networkd.star";
        let src_code = std::fs::read_to_string(file_path)?;

        assert_eq!(
            eval_one_generator(&src_code)?,
            Output {
                files: vec![
                    File {
                        path: "/usr/lib/systemd/network/00-metalos-eth0.network".into(),
                        mode: 0o444,
                        contents: r#"
[Match]
MACAddress=00:00:00:00:00:01

[Network]

Domains=01.abc0.facebook.com abc0.facebook.com facebook.com
# Use static addresses and gw
IPv6AcceptRA=no



[Address]
Address=2a03:2880:f103:181:face:b00c:0:25de/64
PreferredLifetime=forever
[Route]
Gateway=fe80::face:b00c
Source=::/0
Destination=::/0
Metric=10
"#
                        .into()
                    },
                    File {
                        path: "/usr/lib/systemd/network/00-metalos-eth8.network".into(),
                        mode: 0o444,
                        contents: r#"
[Match]
MACAddress=00:00:00:00:00:03

[Network]

Domains=01.abc0.facebook.com abc0.facebook.com facebook.com
# Use static addresses and gw
IPv6AcceptRA=no



"#
                        .into()
                    },
                    File {
                        path: "/usr/lib/systemd/network/00-metalos-beth3.network".into(),
                        mode: 0o444,
                        contents: r#"
[Match]
MACAddress=00:00:00:00:00:04

[Network]

Domains=01.abc0.facebook.com abc0.facebook.com facebook.com
# Use static addresses and gw
IPv6AcceptRA=no



[Address]
Address=2a03:2880:f103:181:face:b00c:a1:0/64
PreferredLifetime=forever
[Route]
Gateway=fe80::face:b00b
Source=2a03:2880:f103:181:face:b00c:a1:0
Destination=::/0
Metric=1024
[Route]
Gateway=fe80::face:b00b
Source=::/0
Destination=::/0
Metric=1026
"#
                        .into()
                    },
                    File {
                        path: "/usr/lib/systemd/network/00-metalos-eth0.link".into(),
                        mode: 0o444,
                        contents: r#"
[Match]
MACAddress=00:00:00:00:00:01

[Link]
NamePolicy=
Name=eth0
MTUBytes=1500
RequiredForOnline=yes
"#
                        .into()
                    },
                    File {
                        path: "/usr/lib/systemd/network/00-metalos-eth8.link".into(),
                        mode: 0o444,
                        contents: r#"
[Match]
MACAddress=00:00:00:00:00:03

[Link]
NamePolicy=
Name=eth8
MTUBytes=1500
RequiredForOnline=no
"#
                        .into()
                    },
                    File {
                        path: "/usr/lib/systemd/network/00-metalos-beth3.link".into(),
                        mode: 0o444,
                        contents: r#"
[Match]
MACAddress=00:00:00:00:00:04

[Link]
NamePolicy=
Name=beth3
MTUBytes=4200
RequiredForOnline=no
"#
                        .into()
                    }
                ],

                dirs: vec![],
                pw_hashes: None,
                zero_files: vec![]
            }
        );
        Ok(())
    }
}
