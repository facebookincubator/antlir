/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeMap;
use std::path::PathBuf;

use antlir2_compile::CompilerContext;
use antlir2_depgraph_if::Requirement;
use antlir2_depgraph_if::Validator;
use antlir2_depgraph_if::item::FileType;
use antlir2_depgraph_if::item::FsEntry;
use antlir2_depgraph_if::item::Item;
use antlir2_depgraph_if::item::ItemKey;
use antlir2_depgraph_if::item::Path as PathItem;
use antlir2_features as _;
use serde::Deserialize;
use serde::Serialize;
use tracing::debug;

pub type Feature = BuildEnvironment;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub struct BuildEnvironment {
    path: String,
}

#[derive(Serialize)]
struct BuildEnvironmentInfo {
    hostname: String,
    env: BTreeMap<String, String>,
    inside_re_worker: bool,
    /// Unprivileged `(uid, gid)` as reported by `antlir2_rootless`
    /// (`Rootless::unprivileged_ids`): the ids a sudo build drops to,
    /// or `None` if the build never ran under sudo.
    unprivileged_ids: Option<(u32, u32)>,
}

const ENV_VARS: &[&str] = &[
    "ACTION_DIGEST",
    "RE_PLATFORM",
    "BUCK_SCRATCH_PATH",
    "TMPDIR",
];

impl antlir2_depgraph_if::RequiresProvides for BuildEnvironment {
    fn provides(&self) -> Result<Vec<Item>, String> {
        Ok(vec![Item::Path(PathItem::Entry(FsEntry {
            path: PathBuf::from(&self.path),
            file_type: FileType::File,
            mode: 0o444,
        }))])
    }

    #[deny(unused_variables)]
    fn requires(&self) -> Result<Vec<Requirement>, String> {
        let path = PathBuf::from(&self.path);
        let mut requires = Vec::new();
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            requires.push(Requirement::ordered(
                ItemKey::Path(parent.to_owned()),
                Validator::FileType(FileType::Directory),
            ));
        }
        Ok(requires)
    }
}

impl antlir2_compile::CompileFeature for BuildEnvironment {
    fn compile(&self, ctx: &CompilerContext) -> antlir2_compile::Result<()> {
        let hostname = nix::unistd::gethostname()
            .map(|h| h.to_string_lossy().into_owned())
            .unwrap_or_else(|_| "unknown".to_owned());

        let mut env = BTreeMap::new();
        for var in ENV_VARS {
            if let Ok(val) = std::env::var(var) {
                // Record absolute paths: the test resolves these from a
                // different cwd, and buck sets BUCK_SCRATCH_PATH relative to
                // the project root.
                let val =
                    std::fs::canonicalize(&val).map_or(val, |p| p.to_string_lossy().into_owned());
                env.insert((*var).to_owned(), val);
            }
        }

        let unprivileged_ids = antlir2_rootless::Rootless::get_if_initialized()
            .and_then(|rootless| {
                let (uid, gid) = rootless.unprivileged_ids();
                uid.zip(gid)
            })
            .map(|(uid, gid)| (uid.as_raw(), gid.as_raw()));

        let info = BuildEnvironmentInfo {
            hostname,
            env,
            inside_re_worker: std::env::var_os("INSIDE_RE_WORKER") == Some("1".into()),
            unprivileged_ids,
        };
        let dst = ctx.dst_path(&self.path)?;
        let json = serde_json::to_string_pretty(&info).map_err(std::io::Error::other)?;
        debug!("Writing build environment to {:?}", dst);
        std::fs::write(&dst, json.as_bytes())?;
        Ok(())
    }
}
