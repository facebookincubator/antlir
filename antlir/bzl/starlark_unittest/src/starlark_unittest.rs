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
use std::path::PathBuf;

use anyhow::anyhow;
use anyhow::Context;
use clap::Parser;
use regex::Regex;
use sha2::Digest;
use sha2::Sha256;
use starlark::any::ProvidesStaticType;
use starlark::environment::FrozenModule;
use starlark::environment::Globals;
use starlark::environment::GlobalsBuilder;
use starlark::environment::LibraryExtension;
use starlark::environment::Module;
use starlark::eval::Evaluator;
use starlark::eval::FileLoader;
use starlark::starlark_module;
use starlark::syntax::AstModule;
use starlark::syntax::Dialect;
use starlark::values::none::NoneType;
use starlark::values::OwnedFrozenValue;
use starlark::values::Value;
use starlark::StarlarkResultExt;
use test::test::ShouldPanic;
use test::test::TestDesc;
use test::test::TestDescAndFn;
use test::test::TestName;
use test::test::TestType;
use test::TestFn;

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
        if a.equals(b)
            .map_err(starlark::Error::into_anyhow)
            .with_context(|| {
                format!(
                    "cannot compare equality of {} and {}",
                    a.to_repr(),
                    b.to_repr()
                )
            })?
        {
            Ok(NoneType)
        } else {
            match msg {
                Some(msg) => Err(anyhow!("{} != {}: {}", a.to_repr(), b.to_repr(), msg)),
                None => Err(anyhow!("{} != {}", a.to_repr(), b.to_repr())),
            }
        }
    }

    fn assert_ne<'v>(a: Value<'v>, b: Value<'v>, msg: Option<&str>) -> anyhow::Result<NoneType> {
        if !a
            .equals(b)
            .map_err(starlark::Error::into_anyhow)
            .with_context(|| {
                format!(
                    "cannot compare equality of {} and {}",
                    a.to_repr(),
                    b.to_repr()
                )
            })?
        {
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
        eval: &mut Evaluator<'v, '_, '_>,
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

#[starlark_module]
fn native(gb: &mut GlobalsBuilder) {
    fn sha256(val: &str) -> anyhow::Result<String> {
        let hash = Sha256::digest(val.as_bytes());
        Ok(hex::encode(hash))
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
                    source_file: "",
                    start_line: 0,
                    start_col: 0,
                    end_line: 0,
                    end_col: 0,
                },
                testfn: TestFn::DynTestFn(Box::new(move || {
                    let module = Module::new();
                    let fail_store = FailStore(RefCell::new(None));
                    let mut evaluator = Evaluator::new(&module);
                    evaluator.extra = Some(&fail_store);
                    evaluator
                        .eval_function(starlark_func.value(), &[], &[])
                        .expect("test function failed");
                    Ok(())
                })),
            })
            .collect()
    }

    fn load(path: PathBuf, loader: &Loader) -> starlark::Result<Self> {
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
struct Loader {
    deps: PathBuf,
    default_cell: String,
}

fn globals() -> Globals {
    GlobalsBuilder::extended_by(&[
        // TODO(nga): drop extensions which are not needed.
        LibraryExtension::StructType,
        LibraryExtension::RecordType,
        LibraryExtension::EnumType,
        LibraryExtension::Map,
        LibraryExtension::Filter,
        LibraryExtension::Partial,
        LibraryExtension::Debug,
        LibraryExtension::Print,
        LibraryExtension::Pprint,
        LibraryExtension::Breakpoint,
        LibraryExtension::Json,
        LibraryExtension::Typing,
    ])
    .with_namespace("unittest", unittest)
    .with(saving_fail)
    .with_namespace("native", native)
    .build()
}

impl FileLoader for Loader {
    fn load(&self, path: &str) -> starlark::Result<FrozenModule> {
        let path = path.trim_start_matches('@');
        if path.starts_with(':') {
            return Err(starlark::Error::new_other(anyhow!(
                "relative loads not allowed: {path}"
            )));
        }
        let path = match path.starts_with("//") {
            true => format!("{}{}", self.default_cell, path),
            false => path.into(),
        };
        let file_path = self.deps.join(path.replace("//", "/"));

        let src = std::fs::read_to_string(&file_path)
            .with_context(|| format!("while reading {}", file_path.display()))?;
        let ast = AstModule::parse(&path, src, &Dialect::Extended)
            .map_err(starlark::Error::into_anyhow)?;

        let module = Module::new();
        {
            let mut eval = Evaluator::new(&module);
            eval.set_loader(self);
            eval.eval_module(ast, &globals())
                .map_err(starlark::Error::into_anyhow)?;
        }
        Ok(module.freeze()?)
    }
}

#[derive(Parser, Debug)]
struct Args {
    #[clap(long)]
    test: Vec<PathBuf>,
    #[clap(long)]
    deps: PathBuf,
    #[clap(long)]
    default_cell: String,
    test_args: Vec<String>,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let loader = Loader {
        deps: args.deps,
        default_cell: args.default_cell,
    };

    let mut modules = vec![];
    for src in args.test {
        let module = TestModule::load(src, &loader).into_anyhow_result()?;
        modules.push(module);
    }

    let tests: Vec<_> = modules.into_iter().flat_map(|m| m.tests()).collect();

    let mut test_args = vec![std::env::args().next().expect("must have at least argv[0]")];
    test_args.extend(args.test_args);
    ::test::test_main(&test_args, tests, None);
    Ok(())
}
