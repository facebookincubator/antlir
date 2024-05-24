/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use nix::sys::stat::Mode;
use serde::Deserialize;
use serde::Serialize;

use crate::item::FileType;
use crate::item::Item;
use crate::item::Path;

/// Requirements are matched by [ItemKey](crate::item::ItemKey) but that does
/// not tell the whole story. Requirements may have additional checks that need
/// to be satisfied. For example, an ItemKey points simply to a Path, but a
/// requirement may require that that Path point to an executable file owned by
/// a certain user. This is difficult to encode directly in the graph, so
/// instead [Validator]s can be added in a Requires edge and checked when
/// finalizing the graph.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Validator {
    /// Always succeeds. Existence of the provider edge is validated when
    /// finalizing the graph.
    Exists,
    /// ANDs all of the contained [Validator]s.
    All(Vec<Validator>),
    /// ORs all of the contained [Validator]s.
    Any(Vec<Validator>),
    /// Assert an [Item] is of a certain [FileType].
    FileType(FileType),
    /// Asserts an [Item] is an executable file.
    Executable,
}

impl Validator {
    pub fn satisfies(&self, item: &Item) -> bool {
        match self {
            Self::Exists => true,
            Self::All(v) => v.iter().all(|v| v.satisfies(item)),
            Self::Any(v) => v.iter().any(|v| v.satisfies(item)),
            Self::FileType(f) => match item {
                Item::Path(Path::Entry(e)) => e.file_type == *f,
                _ => false,
            },
            Self::Executable => match item {
                Item::Path(Path::Entry(e)) => {
                    #[cfg(not(target_os = "macos"))]
                    let mode = Mode::from_bits_truncate(e.mode);
                    #[cfg(target_os = "macos")]
                    let mode = Mode::from_bits_truncate(e.mode as u16);
                    (e.file_type == FileType::File)
                        && (mode.intersects(Mode::S_IXUSR | Mode::S_IXGRP | Mode::S_IXOTH))
                }
                _ => false,
            },
        }
    }
}
