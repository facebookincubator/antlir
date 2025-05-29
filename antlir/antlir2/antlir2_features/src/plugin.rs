/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashMap;
use std::fmt::Debug;
use std::str::FromStr;
use std::sync::Mutex;

use buck_label::Label;
use libloading::Library;
use once_cell::sync::Lazy;

use crate::Error;
use crate::Result;

pub(crate) static REGISTRY: Lazy<Mutex<HashMap<Label, &'static Plugin>>> =
    Lazy::new(Default::default);

/// CLI arg "parser" that immediately loads the plugin libraries and leaks it to
/// remain available for the rest of the process's lifetime
#[derive(Clone)]
pub struct Plugin {
    src: String,
    lib: &'static Library,
}

impl Plugin {
    fn register(src: &str) -> Result<&'static Self> {
        let lib = Box::leak(Box::new(libloading::Library::new(src)?));

        let init_tracing: libloading::Symbol<fn(tracing::Dispatch) -> ()> =
            unsafe { lib.get(b"init_tracing")? };
        init_tracing(tracing::Dispatch::new(PluginForwardingSubscriber));

        let label: libloading::Symbol<fn() -> &'static str> = unsafe { lib.get(b"label\0")? };

        let init_tracing: libloading::Symbol<fn(&tracing::Dispatch) -> ()> =
            unsafe { lib.get(b"init_tracing")? };
        tracing::dispatcher::get_default(|dispatch| {
            init_tracing(dispatch);
        });

        let this = Self {
            src: src.to_owned(),
            lib,
        };

        let plugin = Box::leak(Box::new(this));
        let label = label();
        let label: Label = label
            .parse()
            .map_err(|_| Error::BadPlugin(format!("'{label}' is not a valid label")))?;

        REGISTRY
            .lock()
            .expect("registry lock is poisoned")
            .insert(label, plugin);
        Ok(plugin)
    }

    pub fn get_symbol<T>(&self, symbol: &[u8]) -> Result<libloading::Symbol<T>> {
        unsafe { self.lib.get(symbol).map_err(Error::from) }
    }
}

impl FromStr for Plugin {
    type Err = Error;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        Self::register(s).cloned()
    }
}

impl Debug for Plugin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Plugin").field("src", &self.src).finish()
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
