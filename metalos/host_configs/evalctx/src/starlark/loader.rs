use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::path::Path;
use std::path::PathBuf;

use anyhow::anyhow;
use anyhow::Context;
use starlark::environment::FrozenModule;
use starlark::environment::Module;
use starlark::eval::Evaluator;
use starlark::eval::FileLoader;
use starlark::syntax::AstModule;
use starlark::syntax::Dialect;
use walkdir::WalkDir;

use crate::Error;
use crate::Result;

/// A fully qualified starlark module path. (Starts with '//', final separator
/// is ':')
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ModuleId(PathBuf);

impl ModuleId {
    pub fn new(path: PathBuf) -> Result<Self> {
        if path.parent().is_none() {
            return Err(Error::CreateModule(anyhow!(
                "Path must have a parent to create a valid ModuleId"
            )));
        }

        if path.file_name().is_none() {
            return Err(Error::CreateModule(anyhow!(
                "Path must be a file to to create a valid ModuleId"
            )));
        }

        if path.to_str().is_none() {
            return Err(Error::CreateModule(anyhow!(
                "Path must be UTF-8 to create a valid ModuleId"
            )));
        }

        Ok(Self(path))
    }

    pub fn as_str(&self) -> &str {
        self.0.to_str().expect("all ModuleIds are utf-8")
    }

    /// Convert a canonical id string to a [ModuleId]. Fails if the id is not
    /// canonical in any way.
    pub fn from_id(id: &str) -> Result<Self> {
        match id.strip_prefix("//") {
            Some(path) => {
                if path.matches(':').count() != 1 {
                    return Err(Error::EvalModule(anyhow!(
                        "module id '{}' should contain exactly one ':'",
                        id
                    )));
                }
                Self::new(Path::new("/").join(path.replace(':', "/")))
            }
            None => Err(Error::EvalModule(anyhow!(
                "module id '{}' not absolute",
                id
            ))),
        }
    }

    /// Create a canonical ModuleId from a module path given the root directory
    /// containing all modules.
    pub fn from_path(root: impl AsRef<Path>, module: impl AsRef<Path>) -> Result<Self> {
        let relpath = {
            match module.as_ref().strip_prefix(root) {
                Ok(path) => path,
                Err(_) => return Err(Error::EvalModule(anyhow!("stripping prefix failed"))),
            }
        };

        Self::new(Path::new("/").join(relpath))
    }

    /// Construct an absolute ModuleId from an absolute or relative load().
    pub fn canonicalize_load(&self, load: &str) -> Result<Self> {
        match load.strip_prefix(':') {
            Some(rel) => Ok(Self(self.0.parent().unwrap().join(rel))),
            None => match load.starts_with("//") {
                true => Self::from_id(load),
                false => Err(Error::EvalModule(anyhow!(
                    "load('{}') is neither absolute nor relative",
                    load
                ))),
            },
        }
    }

    /// Absolute path (rooted at the starlark module dir, so is not actually an
    /// absolute filesystem path).
    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl std::fmt::Display for ModuleId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "/{}:{}",
            self.0
                .parent()
                .expect("paths are absolute so have a parent")
                .display(),
            self.0
                .file_name()
                .expect("modules are files so have names")
                .to_string_lossy(),
        )
    }
}

impl std::fmt::Debug for ModuleId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("ModuleId").field(&self.to_string()).finish()
    }
}

pub struct Loader {
    asts: BTreeMap<ModuleId, AstModule>,
    modules: BTreeMap<ModuleId, FrozenModule>,
}

impl Loader {
    /// Recursively load, parse and evaluate all the modules in the given
    /// directory.
    pub fn load(dir: impl AsRef<Path>) -> Result<BTreeMap<ModuleId, FrozenModule>> {
        // parse all the modules into ASTs, then evaluate them in a safe order
        // based on their dependencies
        let files: Vec<_> = WalkDir::new(&dir)
            .into_iter()
            .collect::<walkdir::Result<_>>()
            .map_err(|e| Error::Load(e.into()))?;
        let asts = files
            .into_iter()
            .filter(|e| e.file_type().is_file())
            .filter(|e| e.path().extension() == Some(OsStr::new("star")))
            .map(|e| {
                let src = std::fs::read_to_string(e.path()).map_err(Error::Load)?;
                let id = ModuleId::from_path(&dir, e.path())
                    .context(format!(
                        "unable to created module from dir: {:?} and path: {:?}",
                        dir.as_ref(),
                        e.path(),
                    ))
                    .map_err(Error::CreateModule)?;
                let id_str = id.to_string();
                Ok((
                    id,
                    AstModule::parse(&id_str, src, &Dialect::Extended).map_err(Error::Parse)?,
                ))
            })
            .collect::<Result<_>>()?;

        let loader = Self {
            asts,
            modules: BTreeMap::new(),
        };
        loader.eval_all()
    }

    /// Evaluate all parsed ASTs into frozen modules
    fn eval_all(mut self) -> Result<BTreeMap<ModuleId, FrozenModule>> {
        let keys: Vec<_> = self.asts.keys().cloned().collect();
        for id in keys {
            // this can be None when a module has already been parsed as a
            // dependency of another
            if let Some(ast) = self.asts.remove(&id) {
                self.eval_and_freeze_module(&id, ast)?;
            }
        }
        Ok(self.modules)
    }

    fn eval_and_freeze_module(&mut self, id: &ModuleId, ast: AstModule) -> Result<()> {
        for load in ast.loads() {
            let load_id = id.canonicalize_load(load)?;
            let other_ast = self
                .asts
                .remove(&load_id)
                .ok_or_else(|| Error::MissingImport {
                    src: id.clone(),
                    missing: load_id.clone(),
                })?;
            self.eval_and_freeze_module(&load_id, other_ast)?;
        }
        let module = Module::new();
        let globals = crate::starlark::globals();
        let mut evaluator: Evaluator = Evaluator::new(&module);
        let module_loader = ModuleLoader(id.clone(), &self.modules);
        evaluator.set_loader(&module_loader);
        evaluator
            .eval_module(ast, &globals)
            .map_err(Error::EvalModule)?;

        self.modules
            .insert(id.clone(), module.freeze().map_err(Error::Starlark)?);
        Ok(())
    }
}

struct ModuleLoader<'a>(ModuleId, &'a BTreeMap<ModuleId, FrozenModule>);

impl<'a> FileLoader for ModuleLoader<'a> {
    fn load(&self, load: &str) -> anyhow::Result<FrozenModule> {
        let id = self.0.canonicalize_load(load)?;
        Ok(self
            .1
            .get(&id)
            .with_context(|| format!("'{}' not yet evaluated (or does not exist)", id))?
            .clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use maplit::btreemap;

    impl std::cmp::PartialEq<&str> for ModuleId {
        fn eq(&self, other: &&str) -> bool {
            self.to_string() == *other
        }
    }

    #[test]
    fn module_id() -> Result<()> {
        assert_eq!(
            "//:top.star",
            ModuleId::from_path("/some/dir", "/some/dir/top.star")?.to_string(),
        );
        assert_eq!(
            "//nested/dir:mod.star",
            ModuleId::from_path("/some/dir", "/some/dir/nested/dir/mod.star")?.to_string()
        );
        assert_eq!("//:top.star", ModuleId::from_id("//:top.star")?.to_string());
        assert_eq!(
            "//nested/dir:mod.star",
            ModuleId::from_id("//nested/dir:mod.star")?.to_string()
        );

        let mid = ModuleId::from_id("//nested/dir:mod.star")?;
        assert_eq!(
            "//nested/dir:rel.star",
            mid.canonicalize_load(":rel.star")?.to_string(),
        );
        assert_eq!(
            "//some/abs:mod.star",
            mid.canonicalize_load("//some/abs:mod.star")?.to_string(),
        );
        Ok(())
    }

    #[test]
    fn loads() -> anyhow::Result<()> {
        let asts = btreemap! {
            ModuleId::from_id("//:a.star")? => AstModule::parse("a.star", "load(':b.star', 'x')\ny=x+1".to_string(), &Dialect::Extended)?,
            ModuleId::from_id("//:b.star")? => AstModule::parse("b.star", "x = 42".to_string(), &Dialect::Extended)?,
        };
        let modules = Loader {
            asts,
            modules: BTreeMap::new(),
        }
        .eval_all()?;
        assert_eq!(
            43,
            modules
                .get(&ModuleId::from_id("//:a.star")?)
                .expect("a.star exists")
                .get("y")
                .expect("a.star defines y")
                .unpack_int()
                .expect("y is an int")
        );
        Ok(())
    }

    #[test]
    fn missing_module() -> anyhow::Result<()> {
        let asts = btreemap! {
            ModuleId::from_id("//:a.star")? => AstModule::parse("a.star", "load(':b.star', 'x')".to_string(), &Dialect::Extended)?,
            ModuleId::from_id("//:b.star")? => AstModule::parse("b.star", "x = 42".to_string(), &Dialect::Extended)?,
            ModuleId::from_id("//:c.star")? => AstModule::parse("c.star", "load(':d.star', 'x')".to_string(), &Dialect::Extended)?,
        };
        let err = Loader {
            asts,
            modules: BTreeMap::new(),
        }
        .eval_all()
        .expect_err("d.star should be reported as missing when c.star tries to load it");
        match err {
            Error::MissingImport { src, missing } => {
                assert_eq!(src, "//:c.star");
                assert_eq!(missing, "//:d.star");
            }
            _ => panic!("d.star should be reported as missing when c.star tries to load it"),
        }
        Ok(())
    }
}
