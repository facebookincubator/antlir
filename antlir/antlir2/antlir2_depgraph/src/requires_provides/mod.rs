/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fmt::Debug;

use nix::sys::stat::Mode;
use serde::Deserialize;
use serde::Serialize;

use crate::item::FileType;
use crate::item::Item;
use crate::item::ItemKey;
use crate::item::Path;

mod feature_ext;
pub(crate) use feature_ext::FeatureExt;

pub(crate) struct Requirement<'a> {
    pub(crate) key: ItemKey<'a>,
    pub(crate) validator: Validator<'a>,
    /// This [Requirement] necessitates ordered running of the features
    /// involved. If false, the compiler is free to run the features in any
    /// order.
    pub(crate) ordered: bool,
}

impl<'a> Requirement<'a> {
    /// Hard build dependencies (eg: parent dir exists before install) should
    /// use this function. The compiler will not attempt to build the feature
    /// that has this [Requirement] until the feature that provides it has been
    /// built.
    pub(crate) fn ordered(key: ItemKey<'a>, validator: Validator<'a>) -> Self {
        Self {
            key,
            validator,
            ordered: true,
        }
    }

    /// Logical requirements (eg user's home directory exists) should use this
    /// function. The compiler is free to build the feature that has this
    /// [Requirement] before the feature that provides it, which is useful for
    /// avoiding ordering cycles for purely logical "this has to happen by the
    /// time the layer is done" requirements.
    pub(crate) fn unordered(key: ItemKey<'a>, validator: Validator<'a>) -> Self {
        Self {
            key,
            validator,
            ordered: false,
        }
    }
}

/// Requirements are matched by [ItemKey](crate::item::ItemKey) but that does
/// not tell the whole story. Requirements may have additional checks that need
/// to be satisfied. For example, an ItemKey points simply to a Path, but a
/// requirement may require that that Path point to an executable file owned by
/// a certain user. This is difficult to encode directly in the graph, so
/// instead [Validator]s can be added in a Requires edge and checked when
/// finalizing the graph.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case", bound(deserialize = "'de: 'a"))]
pub enum Validator<'a> {
    /// Always succeeds. Existence of the provider edge is validated when
    /// finalizing the graph.
    Exists,
    /// Validator that succeeds if the [Item] signifies a removal of a previous
    /// item. The depgraph validation special-cases this to pass when the
    /// requirement is a [crate::Node::MissingItem].
    DoesNotExist,
    /// ANDs all of the contained [Validator]s.
    All(Vec<Validator<'a>>),
    /// ORs all of the contained [Validator]s.
    Any(Vec<Validator<'a>>),
    /// Assert an [Item] is of a certain [FileType].
    FileType(FileType),
    /// Asserts an [Item] is an executable file.
    Executable,
    /// Assert that an [ItemKey] within another layer matches some [Validator]
    ItemInLayer {
        key: ItemKey<'a>,
        validator: Box<Validator<'a>>,
    },
}

impl<'a> Validator<'a> {
    pub(crate) fn satisfies(&self, item: &Item<'_>) -> bool {
        match self {
            Self::Exists => true,
            Self::DoesNotExist => match item {
                Item::Path(_) => false,
                _ => false,
            },
            Self::All(v) => v.iter().all(|v| v.satisfies(item)),
            Self::Any(v) => v.iter().any(|v| v.satisfies(item)),
            Self::FileType(f) => match item {
                Item::Path(Path::Entry(e)) => e.file_type == *f,
                _ => false,
            },
            Self::Executable => match item {
                Item::Path(Path::Entry(e)) => {
                    let mode = Mode::from_bits_truncate(e.mode);
                    (e.file_type == FileType::File)
                        && (mode.intersects(Mode::S_IXUSR | Mode::S_IXGRP | Mode::S_IXOTH))
                }
                _ => false,
            },
            Self::ItemInLayer { key, validator } => match item {
                Item::Layer(layer) => match layer.graph.items.get(key) {
                    Some(item_in_layer_nx) => {
                        validator.satisfies(&layer.graph.g[*item_in_layer_nx])
                    }
                    None => &Self::DoesNotExist == validator.as_ref(),
                },
                _ => false,
            },
        }
    }
}
