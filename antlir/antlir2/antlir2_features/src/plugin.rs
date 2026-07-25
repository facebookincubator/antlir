/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashMap;
use std::fmt::Debug;
use std::str::FromStr;
use std::sync::LazyLock;
use std::sync::Mutex;

use buck_label::Label;
use libloading::Library;

use crate::Error;
use crate::Result;

pub(crate) static REGISTRY: LazyLock<Mutex<HashMap<Label, &'static Plugin>>> =
    LazyLock::new(Default::default);

/// Loaded plugin library, registered in `REGISTRY`.
pub struct Plugin {
    src: String,
    lib: &'static Library,
}

/// CLI argument that holds a path to a plugin .so without loading it.
///
/// Loading is deferred to an explicit `load` step so it happens after
/// the host tracing subscriber is installed. `FromStr` is invoked during clap
/// parsing, before `registry().init()`, so doing I/O or touching the tracing
/// global there would run too early. Requiring an explicit `load` with a
/// `Dispatch` token makes the ordering explicit.
#[derive(Clone, Debug)]
pub struct PluginArg {
    src: String,
}

impl FromStr for PluginArg {
    type Err = Error;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        Ok(Self { src: s.to_owned() })
    }
}

impl PluginArg {
    /// Load this plugin, installing the provided host `dispatch` into
    /// the plugin's address space.
    ///
    /// By convention `dispatch` should be obtained via
    /// `tracing::Dispatch::default()` after `registry().init()` has run, so
    /// that the host's real subscriber is forwarded to the plugin. This is
    /// only a calling convention, not a runtime guarantee: if `load` is
    /// invoked before tracing is initialized, `Dispatch::default()` yields a
    /// no-op dispatcher and the plugin silently gets no-op tracing.
    pub fn load(self, dispatch: tracing::Dispatch) -> Result<&'static Plugin> {
        Plugin::register(self.src, dispatch)
    }
}

impl Plugin {
    fn register(src: String, dispatch: tracing::Dispatch) -> Result<&'static Self> {
        let lib: Box<Library> = Box::new(libloading::Library::new(&src)?);

        let label: libloading::Symbol<fn() -> &'static str> = unsafe { lib.get(b"label\0")? };
        let label = label();
        let label: Label = label
            .parse()
            .map_err(|_| Error::BadPlugin(format!("'{label}' is not a valid label")))?;

        match unsafe { lib.get::<fn(tracing::Dispatch)>(b"init_tracing\0") } {
            Ok(init_tracing) => init_tracing(dispatch),
            Err(e) => tracing::warn!(
                "plugin '{src}' does not export 'init_tracing'; \
                 tracing will not be forwarded to it: {e}"
            ),
        }

        // Setup is complete; leak the already-heap-allocated library so that
        // it's guaranteed to live for the rest of the process's lifetime, when
        // it might otherwise be unloaded while some unknown references still
        // exist in various features
        let lib: &'static Library = Box::leak(lib);
        let this = Self { src, lib };

        let plugin = Box::leak(Box::new(this));

        REGISTRY
            .lock()
            .expect("registry lock is poisoned")
            .insert(label, plugin);
        Ok(plugin)
    }

    /// Bulk-load a list of plugin args with the same host dispatch.
    pub fn load_all(
        args: Vec<PluginArg>,
        dispatch: tracing::Dispatch,
    ) -> Result<Vec<&'static Self>> {
        args.into_iter().map(|a| a.load(dispatch.clone())).collect()
    }

    pub fn get_symbol<T>(&self, symbol: &[u8]) -> Result<libloading::Symbol<T>> {
        unsafe { self.lib.get(symbol).map_err(Error::from) }
    }
}

impl Debug for Plugin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Plugin").field("src", &self.src).finish()
    }
}
