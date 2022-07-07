/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#![feature(test)]
extern crate test;

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use test::test::ShouldPanic;
use test::test::TestDesc;
use test::test::TestDescAndFn;
use test::test::TestName;
use test::test::TestType;
use test::TestFn;

use absolute_path::AbsolutePath;
use anyhow::anyhow;
use anyhow::Context;
use anyhow::Result;
use clap::Parser;
use gazebo::any::ProvidesStaticType;
use regex::Regex;
use starlark::environment::FrozenModule;
use starlark::environment::Globals;
use starlark::environment::GlobalsBuilder;
use starlark::environment::Module;
use starlark::eval::Evaluator;
use starlark::eval::FileLoader;
use starlark::starlark_module;
use starlark::syntax::AstModule;
use starlark::syntax::Dialect;
use starlark::values::none::NoneType;
use starlark::values::OwnedFrozenValue;
use starlark::values::Value;

#[derive(Debug, ProvidesStaticType, Default)]
struct FailStore(RefCell<Option<String>>);

impl FailStore {
    fn set(&self, x: String) {
        self.0.replace(Some(x));
    }
    fn take(&self) -> Option<String> {
        self.0.replace(None)
    }
}

#[starlark_module]
fn saving_fail(gb: &mut GlobalsBuilder) {
    /// Replaces the global fail() function to save the raw error message in eval.extra
    fn fail(msg: &str, eval: &mut Evaluator) -> anyhow::Result<NoneType> {
        eval.extra
            .context("eval.extra missing")?
            .downcast_ref::<FailStore>()
            .context("eval.extra not FailStore")?
            .set(msg.to_string());
        Err(anyhow!("fail: {}", msg))
    }
}

/// Starlark interface used by .star unit test files
#[starlark_module]
fn unittest(gb: &mut GlobalsBuilder) {
    fn assert_eq<'v>(a: Value<'v>, b: Value<'v>, msg: Option<&str>) -> anyhow::Result<NoneType> {
        if a.equals(b).with_context(|| {
            format!(
                "cannot compare equality of {} and {}",
                a.to_repr(),
                b.to_repr()
            )
        })? {
            Ok(NoneType)
        } else {
            match msg {
                Some(msg) => Err(anyhow!("{} != {}: {}", a.to_repr(), b.to_repr(), msg)),
                None => Err(anyhow!("{} != {}", a.to_repr(), b.to_repr())),
            }
        }
    }

    fn assert_ne<'v>(a: Value<'v>, b: Value<'v>, msg: Option<&str>) -> anyhow::Result<NoneType> {
        if !a.equals(b).with_context(|| {
            format!(
                "cannot compare equality of {} and {}",
                a.to_repr(),
                b.to_repr()
            )
        })? {
            Ok(NoneType)
        } else {
            match msg {
                Some(msg) => Err(anyhow!("{} == {}: {}", a.to_repr(), b.to_repr(), msg)),
                None => Err(anyhow!("{} == {}", a.to_repr(), b.to_repr())),
            }
        }
    }

    /// Run the given function (usually created with `partial()`) and assert
    /// that it calls 'fail' with a message that passes the input regex
    fn assert_fails<'v>(
        func: Value<'v>,
        message_regex: &str,
        eval: &mut Evaluator<'v, '_>,
    ) -> anyhow::Result<NoneType> {
        match eval.eval_function(func, &[], &[]) {
            Ok(_) => Err(anyhow!("function did not fail")),
            Err(_) => match eval.extra {
                None => Err(anyhow!("eval.extra does not have failure context")),
                Some(store) => {
                    let msg = store
                        .downcast_ref::<FailStore>()
                        .context("eval.extra is not FailStore")?
                        .take()
                        .context("fail error message missing")?;
                    let re = Regex::new(message_regex).context("invalid regex")?;
                    match re.is_match(&msg) {
                        true => Ok(NoneType),
                        false => Err(anyhow!("{} did not match the pattern '{}'", msg, re)),
                    }
                }
            },
        }
    }
}

#[derive(Debug, Clone)]
struct TestModule {
    name: String,
    module: FrozenModule,
}

impl TestModule {
    fn tests(&self) -> Vec<TestDescAndFn> {
        let test_names = self
            .module
            .names()
            .filter(|n| n.as_str().starts_with("test_"));
        let tests: HashMap<String, OwnedFrozenValue> = test_names
            .into_iter()
            .map(|name| {
                (
                    name.as_str().to_string(),
                    self.module
                        .get(&name)
                        .expect("couldn't get test out of module"),
                )
            })
            .collect();

        tests
            .into_iter()
            .map(|(name, starlark_func)| TestDescAndFn {
                desc: TestDesc {
                    name: TestName::DynTestName(format!("{}/{}", self.name, name)),
                    ignore: false,
                    ignore_message: None,
                    should_panic: ShouldPanic::No,
                    compile_fail: false,
                    no_run: false,
                    test_type: TestType::UnitTest,
                },
                testfn: TestFn::DynTestFn(Box::new(move || {
                    let module = Module::new();
                    let mut evaluator = Evaluator::new(&module);
                    let fail_store = FailStore(RefCell::new(None));
                    evaluator.extra = Some(&fail_store);
                    evaluator
                        .eval_function(starlark_func.value(), &[], &[])
                        .expect("test function failed");
                })),
            })
            .collect()
    }

    fn load(path: PathBuf, loader: &Loader) -> Result<Self> {
        let name = path
            .file_name()
            .context("modules always have filenames")?
            .to_str()
            .context("name is always string")?
            .to_string();

        let module = loader.load(path.to_str().context("while converting path to str")?)?;
        Ok(Self { name, module })
    }
}

#[derive(Debug)]
struct Loader(HashMap<String, PathBuf>);

fn globals() -> Globals {
    GlobalsBuilder::extended()
        .with_struct("unittest", unittest)
        .with(saving_fail)
        .build()
}

impl FileLoader for Loader {
    fn load(&self, path: &str) -> Result<FrozenModule> {
        let path = path.trim_start_matches('@');
        let file_path = self
            .0
            .get(path)
            .map_or_else(|| Path::new(path), PathBuf::as_path);

        let file_path = match file_path.is_absolute() {
            true => file_path.to_path_buf(),
            false => {
                let repo_root = find_root::find_repo_root(
                    AbsolutePath::new(
                        &std::env::current_exe().expect("current_exe is always here"),
                    )
                    .expect("current_exe must be absolute"),
                )
                .context("could not find repo root")?;
                repo_root.join(file_path)
            }
        };

        let src = std::fs::read_to_string(&file_path)
            .with_context(|| format!("while reading {}", file_path.display()))?;
        let ast = AstModule::parse(path, src, &Dialect::Extended)?;

        let module = Module::new();
        let mut eval = Evaluator::new(&module);
        eval.set_loader(self);
        eval.eval_module(ast, &globals())?;
        module.freeze()
    }
}

#[derive(Parser, Debug)]
struct Args {
    #[clap(long)]
    targets_and_outputs: PathBuf,
    test_srcs: PathBuf,
    test_args: Vec<String>,
}

fn main() -> Result<()> {
    let args = Args::parse();

    let deps = std::fs::read(&args.targets_and_outputs)
        .with_context(|| format!("while reading {}", args.targets_and_outputs.display()))?;
    let deps = serde_json::from_slice(&deps)
        .with_context(|| format!("while parsing {}", args.targets_and_outputs.display()))
        .map(Loader)?;

    let mut modules = vec![];
    for entry in std::fs::read_dir(&args.test_srcs)
        .with_context(|| format!("while looking for sources in {}", args.test_srcs.display()))?
    {
        let path = entry?.path().to_path_buf();
        let module = TestModule::load(path, &deps)?;
        modules.push(module);
    }

    let tests: Vec<_> = modules.into_iter().flat_map(|m| m.tests()).collect();

    let mut test_args = vec![std::env::args().next().unwrap()];
    test_args.extend(args.test_args);
    ::test::test_main(&test_args, tests, None);
    Ok(())
}