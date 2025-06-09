/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::Path;
use std::process::Command;

use anyhow::Context;
use anyhow::Result;
use serde::Deserialize;
use serde::Serialize;
use strum::Display;

#[derive(Debug, PartialEq, Eq, Clone, Deserialize, Serialize)]
pub struct UnitFile {
    #[serde(rename = "unit_file")]
    name: String,
    state: UnitFileState,
    #[serde(default)]
    preset: Option<Preset>,
}

impl UnitFile {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn state(&self) -> UnitFileState {
        self.state
    }

    pub fn preset(&self) -> Option<Preset> {
        self.preset
    }

    pub fn unit_type(&self) -> UnitType {
        let (_, ty) = self.name.rsplit_once('.').expect("unit always has suffix");
        ty.into()
    }
}

/// Install state of a unit file.
#[derive(Debug, PartialEq, Eq, Copy, Clone, Deserialize, Serialize, Display)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
pub enum UnitFileState {
    /// Unit file is permanently enabled.
    Enabled,
    /// Unit file is only temporarily enabled and will no longer be enabled
    /// after a reboot (that means, it is enabled via /run/ symlinks, rather
    /// than /etc/).
    EnabledRuntime,
    /// Unit file is linked into /etc/ permanently.
    Linked,
    /// Unit file is linked into /run/ temporarily (until the next reboot).
    LinkedRuntime,
    /// Unit file is masked permanently.
    Masked,
    /// Unit file is masked in /run/ temporarily (until the next reboot).
    MaskedRuntime,
    /// Unit is statically enabled. i.e. always enabled and doesn't need to be
    /// enabled explicitly.
    Static,
    /// Unit file is not enabled.
    Disabled,
    /// It could not be determined whether the unit file is enabled.
    Invalid,
    /// Unit file is symlinked so it can be referred to by another name.
    Alias,
    /// The unit file itself is not enabled, but it has a non-empty Also=
    /// setting in the [Install] unit file section, listing other unit files
    /// that might be enabled, or it has an alias under a different name through
    /// a symlink that is not specified in Also=. For template unit files, an
    /// instance different than the one specified in DefaultInstance= is
    /// enabled.
    Indirect,
    Generated,
    Bad,
}

#[derive(Debug, PartialEq, Eq, Copy, Clone, Deserialize, Serialize, Display)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum Preset {
    Enabled,
    Disabled,
}

#[derive(Debug, PartialEq, Eq, Copy, Clone, Deserialize, Serialize, Display)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum UnitType<'a> {
    Service,
    Socket,
    Device,
    Mount,
    Automount,
    Swap,
    Target,
    Path,
    Timer,
    Slice,
    Scope,
    Other(&'a str),
}

impl<'a> From<&'a str> for UnitType<'a> {
    fn from(s: &'a str) -> Self {
        match s {
            "service" => Self::Service,
            "socket" => Self::Socket,
            "device" => Self::Device,
            "mount" => Self::Mount,
            "automount" => Self::Automount,
            "swap" => Self::Swap,
            "target" => Self::Target,
            "path" => Self::Path,
            "timer" => Self::Timer,
            "slice" => Self::Slice,
            "scope" => Self::Slice,
            s => Self::Other(s),
        }
    }
}

pub fn list_unit_files<P>(root: P) -> Result<Vec<UnitFile>>
where
    P: AsRef<Path>,
{
    let out = Command::new("systemctl")
        .arg("--output=json")
        .arg("--root")
        .arg(root.as_ref())
        .arg("list-unit-files")
        .output()
        .context("while running 'systemctl list-unit-files'")?;
    // systemd can return a non-zero exit code if there are no unit files, while
    // still printing out valid json, so just don't bother checking the error
    // code
    serde_json::from_slice(&out.stdout).context("while deserializing unit files")
}
