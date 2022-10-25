/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fs::File;
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
    #[error(transparent)]
    AbsolutePath(absolute_path::Error),
    #[error("For file {0} JSON Parse failed because {1}")]
    JSONParseError(PathBuf, anyhow::Error),
    #[error("both path_in_repo and subvolumes_dir are provided")]
    MultipleSourcesOfTruth,
    #[error(transparent)]
    BuckCellRoot(find_root::FindRootError),
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
    subvolumes_dir: Option<AbsolutePathBuf>,
    path_in_repo: Option<AbsolutePathBuf>,
) -> Result<AbsolutePathBuf> {
    if subvolumes_dir.is_some() && path_in_repo.is_some() {
        return Err(FindBuiltSubvolError::MultipleSourcesOfTruth);
    }
    let json_file_path = layer_output.join("layer.json");

    let subvolumes_dir_rel_path = Path::new("buck-image-out/volume/targets");

    let layer_info = get_layer_info(&json_file_path)?;

    let subvolumes_dir: AbsolutePathBuf = match subvolumes_dir {
        Some(dir) => dir,
        None => match path_in_repo {
            Some(path) => AbsolutePathBuf::new(path.join(subvolumes_dir_rel_path))
                .map_err(FindBuiltSubvolError::AbsolutePath)?,

            None => return Err(FindBuiltSubvolError::NoSourceOfTruth),
        },
    };

    let target_subvol_path = subvolumes_dir.join(layer_info.subvolume_rel_path);

    AbsolutePathBuf::new(target_subvol_path).map_err(FindBuiltSubvolError::AbsolutePath)
}
