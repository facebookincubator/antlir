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
#[serde(bound(deserialize = "'de: 'a"))]
pub struct Requirement<'a> {
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
    pub fn ordered(key: ItemKey<'a>, validator: Validator<'a>) -> Self {
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
    pub fn unordered(key: ItemKey<'a>, validator: Validator<'a>) -> Self {
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
    fn provides(&self) -> std::result::Result<Vec<Item<'static>>, String>;
    fn requires(&self) -> std::result::Result<Vec<Requirement<'static>>, String>;
}

/// PluginExt indirects the implementation of [RequiresProvides] through a .so
/// plugin. The underlying crates all provide a type that implements
/// [RequiresProvides], and some generated code provides a set of exported
/// symbols that let us call that implementation.
trait PluginExt {
    fn provides_fn(
        &self,
    ) -> Result<libloading::Symbol<fn(&Feature) -> Result<Vec<Item<'static>>, String>>, String>;

    fn requires_fn(
        &self,
    ) -> Result<libloading::Symbol<fn(&Feature) -> Result<Vec<Requirement<'static>>, String>>, String>;
}

impl PluginExt for antlir2_features::Plugin {
    fn provides_fn(
        &self,
    ) -> Result<libloading::Symbol<fn(&Feature) -> Result<Vec<Item<'static>>, String>>, String>
    {
        self.get_symbol(b"RequiresProvides_provides\0")
            .map_err(|e| format!("failed to get provides fn: {e}"))
    }

    fn requires_fn(
        &self,
    ) -> Result<libloading::Symbol<fn(&Feature) -> Result<Vec<Requirement<'static>>, String>>, String>
    {
        self.get_symbol(b"RequiresProvides_requires\0")
            .map_err(|e| format!("failed to get provides fn: {e}"))
    }
}

impl RequiresProvides for Feature {
    #[tracing::instrument]
    fn provides(&self) -> std::result::Result<Vec<Item<'static>>, String> {
        self.plugin().map_err(|e| e.to_string())?.provides_fn()?(self)
    }

    #[tracing::instrument]
    fn requires(&self) -> std::result::Result<Vec<Requirement<'static>>, String> {
        self.plugin().map_err(|e| e.to_string())?.requires_fn()?(self)
    }
}
