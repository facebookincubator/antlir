/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::Path;
use std::path::PathBuf;

pub(super) fn canonicalized_bind(src: &Path, dst: &Path) -> std::io::Result<(PathBuf, PathBuf)> {
    let canonical_src = src.canonicalize()?;
    if src == dst {
        Ok((canonical_src.clone(), canonical_src))
    } else {
        let canonical_dst = match dst.is_absolute() {
            true => dst.to_owned(),
            false => std::env::current_dir().map(|cwd| cwd.join(dst))?,
        };
        Ok((canonical_src, canonical_dst))
    }
}
