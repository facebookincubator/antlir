/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::hash::Hash;
use std::path::Path;
use std::path::PathBuf;

use serde::Deserialize;
use serde::Serialize;

use crate::Error;
use crate::Result;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) struct PluginJson {
    pub(crate) plugin: PathBuf,
    libs: PathBuf,
}

pub struct Plugin {
    path: PathBuf,
    lib: &'static libloading::Library,
}

impl Plugin {
    pub(crate) fn open(path: &Path) -> Result<Self> {
        let lib = Box::leak(Box::new(libloading::Library::new(path)?));
        let init_tracing: libloading::Symbol<fn(tracing::Dispatch) -> ()> =
            unsafe { lib.get(b"init_tracing")? };
        init_tracing(tracing::Dispatch::new(PluginForwardingSubscriber));

        Ok(Self {
            path: path.to_owned(),
            lib,
        })
    }

    pub fn get_symbol<T>(&self, symbol: &[u8]) -> Result<libloading::Symbol<T>> {
        unsafe { self.lib.get(symbol).map_err(Error::from) }
    }
}

impl PartialEq for Plugin {
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path
    }
}

impl Eq for Plugin {}

impl std::fmt::Debug for Plugin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Plugin")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

/// A [tracing::Subscriber] implementation that simply forwards every call to
/// the default dispatcher in this library's "symbol space", since otherwise the
/// plugin has a separate copy of the GLOBAL_DISPATCH static that is not
/// connected to the "main" dispatcher found in the rust_binary's
///
/// Historically, antlir2 plugins used to receive the default dispatcher
/// whenever they were loaded, but that was subject to annoying ordering issues
/// if the default dispatcher was registered/changed after the plugin had been
/// loaded.
struct PluginForwardingSubscriber;

macro_rules! forward_to_default_dispatch {
    ($(fn $func_name:ident(&self $(, $arg:ident: $arg_ty:ty)*) -> $r:ty;)*) => {
        $(
            fn $func_name(&self $(, $arg: $arg_ty)*) -> $r {
                tracing::dispatcher::get_default(move |dispatch| {
                    dispatch.$func_name($($arg),*)
                })
            }
        )*
    };
}

use tracing_core::span;

impl tracing::Subscriber for PluginForwardingSubscriber {
    forward_to_default_dispatch! {
        fn enabled(&self, metadata: &tracing::Metadata<'_>) -> bool;
        fn new_span(&self, span: &span::Attributes<'_>) -> span::Id;
        fn record(&self, span: &span::Id, values: &span::Record<'_>) -> ();
        fn record_follows_from(&self, span: &span::Id, follows: &span::Id) -> ();
        fn event(&self, event: &tracing::Event<'_>) -> ();
        fn enter(&self, span: &span::Id) -> ();
        fn exit(&self, span: &span::Id) -> ();
        fn register_callsite(&self, metadata: &'static tracing::Metadata<'static>) -> tracing::subscriber::Interest;
        fn clone_span(&self, id: &span::Id) -> span::Id;
        fn current_span(&self) -> span::Current;
    }
    fn drop_span(&self, id: span::Id) {
        tracing::dispatcher::get_default(move |dispatch| {
            #[allow(deprecated)]
            dispatch.drop_span(id.clone())
        })
    }
    fn try_close(&self, id: span::Id) -> bool {
        tracing::dispatcher::get_default(move |dispatch| dispatch.try_close(id.clone()))
    }
}
