/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fmt::Debug;

use antlir2_features::Feature;
use nix::sys::stat::Mode;
use serde::Deserialize;
use serde::Serialize;

use crate::item::FileType;
use crate::item::Item;
use crate::item::ItemKey;
use crate::item::Path;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct Requirement {
    pub(crate) key: ItemKey,
    pub(crate) validator: Validator,
    /// This [Requirement] necessitates ordered running of the features
    /// involved. If false, the compiler is free to run the features in any
    /// order.
    pub(crate) ordered: bool,
}

impl Requirement {
    /// Hard build dependencies (eg: parent dir exists before install) should
    /// use this function. The compiler will not attempt to build the feature
    /// that has this [Requirement] until the feature that provides it has been
    /// built.
    pub fn ordered(key: ItemKey, validator: Validator) -> Self {
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
    pub fn unordered(key: ItemKey, validator: Validator) -> Self {
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
#[serde(rename_all = "snake_case")]
pub enum Validator {
    /// Always succeeds. Existence of the provider edge is validated when
    /// finalizing the graph.
    Exists,
    /// Validator that succeeds if the [Item] signifies a removal of a previous
    /// item. The depgraph validation special-cases this to pass when the
    /// requirement is a [crate::Node::MissingItem].
    DoesNotExist,
    /// ANDs all of the contained [Validator]s.
    All(Vec<Validator>),
    /// ORs all of the contained [Validator]s.
    Any(Vec<Validator>),
    /// Assert an [Item] is of a certain [FileType].
    FileType(FileType),
    /// Asserts an [Item] is an executable file.
    Executable,
    /// Assert that an [ItemKey] within another layer matches some [Validator]
    ItemInLayer {
        key: ItemKey,
        validator: Box<Validator>,
    },
}

impl Validator {
    pub(crate) fn satisfies(&self, item: &Item) -> bool {
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
                    #[cfg(not(target_os = "macos"))]
                    let mode = Mode::from_bits_truncate(e.mode);
                    #[cfg(target_os = "macos")]
                    let mode = Mode::from_bits_truncate(e.mode as u16);
                    (e.file_type == FileType::File)
                        && (mode.intersects(Mode::S_IXUSR | Mode::S_IXGRP | Mode::S_IXOTH))
                }
                _ => false,
            },
            Self::ItemInLayer { key, validator } => match item {
                Item::Layer(layer) => match layer.graph.get_item(key) {
                    Some(item_in_layer) => validator.satisfies(item_in_layer),
                    None => &Self::DoesNotExist == validator.as_ref(),
                },
                _ => false,
            },
        }
    }
}

pub trait RequiresProvides {
    fn provides(&self) -> std::result::Result<Vec<Item>, String>;
    fn requires(&self) -> std::result::Result<Vec<Requirement>, String>;
}

/// PluginExt indirects the implementation of [RequiresProvides] through a .so
/// plugin. The underlying crates all provide a type that implements
/// [RequiresProvides], and some generated code provides a set of exported
/// symbols that let us call that implementation.
trait PluginExt {
    fn provides_fn(
        &self,
    ) -> Result<libloading::Symbol<fn(&Feature) -> Result<Vec<Item>, String>>, String>;

    fn requires_fn(
        &self,
    ) -> Result<libloading::Symbol<fn(&Feature) -> Result<Vec<Requirement>, String>>, String>;
}

impl PluginExt for antlir2_features::Plugin {
    fn provides_fn(
        &self,
    ) -> Result<libloading::Symbol<fn(&Feature) -> Result<Vec<Item>, String>>, String> {
        self.get_symbol(b"RequiresProvides_provides\0")
            .map_err(|e| format!("failed to get provides fn: {e}"))
    }

    fn requires_fn(
        &self,
    ) -> Result<libloading::Symbol<fn(&Feature) -> Result<Vec<Requirement>, String>>, String> {
        self.get_symbol(b"RequiresProvides_requires\0")
            .map_err(|e| format!("failed to get provides fn: {e}"))
    }
}

impl RequiresProvides for Feature {
    #[tracing::instrument]
    fn provides(&self) -> std::result::Result<Vec<Item>, String> {
        self.plugin().map_err(|e| e.to_string())?.provides_fn()?(self)
    }

    #[tracing::instrument]
    fn requires(&self) -> std::result::Result<Vec<Requirement>, String> {
        self.plugin().map_err(|e| e.to_string())?.requires_fn()?(self)
    }
}
