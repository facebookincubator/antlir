/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::ffi::OsStr;
use std::fs::File;
use std::path::Component;
use std::path::Path;
use std::path::PathBuf;

use absolute_path::AbsolutePathBuf;
use serde::Deserialize;

pub type Result<R> = std::result::Result<R, FindBuiltSubvolError>;

#[derive(Deserialize, Debug)]
pub struct LayerInfo {
    subvolume_rel_path: PathBuf,
}

#[derive(Debug, thiserror::Error)]
pub enum FindBuiltSubvolError {
    #[error("For file {0} JSON Parse failed because {1}")]
    JSONParseError(PathBuf, anyhow::Error),
    #[error("more than one of subvolumes_dir, buck_cell_root, and path_in_repo provided")]
    MultipleSourcesOfTruth,
    #[error(transparent)]
    BuckCellRoot(find_root::FindRootError),
    #[error("failed to absolutize path: {0}")]
    AbsolutePathError(#[from] absolute_path::Error),
    #[error("there is no parent dir to find the subvol from")]
    NoSourceOfTruth,
}

fn get_layer_info(json_file_path: &Path) -> Result<LayerInfo> {
    let json_file_path = json_file_path.to_path_buf();
    let file = File::open(&json_file_path)
        .map_err(|e| FindBuiltSubvolError::JSONParseError(json_file_path.clone(), e.into()))?;
    let mapped: LayerInfo = serde_json::from_reader(file)
        .map_err(|e| FindBuiltSubvolError::JSONParseError(json_file_path.clone(), e.into()))?;
    Ok(mapped)
}

pub fn find_built_subvol(
    layer_output: AbsolutePathBuf,
    // Absolute path to the local dir where built Antlir subvolumes are stored
    subvolumes_dir: Option<AbsolutePathBuf>,
    // Absolute path to the root of the Buck cell, used to find the subvolumes dir.
    buck_cell_root: Option<AbsolutePathBuf>,
    // Any arbitrary path in the repo where built Antlir subvolumes are also stored,
    // which is thus used to derive the path to this storage dir.
    path_in_repo: Option<PathBuf>,
) -> Result<AbsolutePathBuf> {
    match (&subvolumes_dir, &buck_cell_root, &path_in_repo) {
        (Some(_), Some(_), _) | (Some(_), _, Some(_)) | (_, Some(_), Some(_)) => {
            Err(FindBuiltSubvolError::MultipleSourcesOfTruth)
        }
        _ => Ok(()),
    }?;

    if let Ok(target) = std::fs::read_link(&layer_output) {
        if target
            .components()
            .any(|comp| comp == Component::Normal(OsStr::new("antlir2-out")))
        {
            return Ok(target.try_into()?);
        }
    }

    let json_file_path = layer_output.join("layer.json");
    let subvolumes_dir_rel_path = Path::new("buck-image-out/volume/targets");
    let layer_info = get_layer_info(&json_file_path)?;
    let subvol_rel_path = layer_info.subvolume_rel_path;

    if let Some(sv_dir) = subvolumes_dir {
        return Ok(sv_dir.join(subvol_rel_path));
    }

    if let Some(root) = buck_cell_root {
        return Ok(root.join(subvolumes_dir_rel_path).join(subvol_rel_path));
    }

    if let Some(repo_path) = path_in_repo {
        let abs_path_in_repo = AbsolutePathBuf::absolutize(repo_path)
            .map_err(FindBuiltSubvolError::AbsolutePathError)?;
        let buck_root = find_root::find_buck_cell_root(&abs_path_in_repo)
            .map_err(FindBuiltSubvolError::BuckCellRoot)?;
        return Ok(buck_root
            .join(subvolumes_dir_rel_path)
            .join(subvol_rel_path));
    }

    Err(FindBuiltSubvolError::NoSourceOfTruth)
}

#[cfg(test)]
mod tests {
    use anyhow::anyhow;
    use anyhow::Result;

    use super::*;

    #[test]
    fn test_find_built_subvol_bad_args() -> Result<()> {
        let layer_out: AbsolutePathBuf = PathBuf::from("/not/used").try_into()?;
        let abspath: Option<AbsolutePathBuf> = Some(PathBuf::from("/foo").try_into()?);
        let pathbuf = Some(PathBuf::from("/bar"));
        for (a, b, c) in vec![
            (abspath.clone(), abspath.clone(), None),
            (abspath.clone(), None, pathbuf.clone()),
            (None, abspath.clone(), pathbuf.clone()),
            (abspath.clone(), abspath, pathbuf),
        ] {
            match find_built_subvol(layer_out.clone(), a, b, c) {
                Err(e) => match e {
                    FindBuiltSubvolError::MultipleSourcesOfTruth => Ok(()),
                    res => Err(anyhow!("function returned unexepcted result: {:#?}", res)),
                },
                res => Err(anyhow!("function returned unexepcted result: {:#?}", res)),
            }?
        }
        Ok(())
    }
}
