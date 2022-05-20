/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum SendVersion {
    SendVersion1 = 1,
    SendVersion2 = 2,
}

impl TryFrom<u32> for SendVersion {
    type Error = anyhow::Error;
    fn try_from(value: u32) -> anyhow::Result<SendVersion> {
        match value {
            x if x == SendVersion::SendVersion1 as u32 => Ok(SendVersion::SendVersion1),
            x if x == SendVersion::SendVersion2 as u32 => Ok(SendVersion::SendVersion2),
            _ => anyhow::bail!("Invalid version {}", value),
        }
    }
}

impl std::fmt::Display for SendVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SendVersion::SendVersion1 => write!(f, "SendVersion1"),
            SendVersion::SendVersion2 => write!(f, "SendVersion2"),
        }
    }
}
