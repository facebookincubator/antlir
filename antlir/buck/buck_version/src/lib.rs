/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use clap::ValueEnum;
use serde::de::Error as _;
use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;
use serde::Serializer;

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, ValueEnum)]
pub enum BuckVersion {
    #[clap(name = "1")]
    One,
    #[clap(name = "2")]
    Two,
}

impl BuckVersion {
    /// Command that will give you this version of buck for use in subprocesses
    pub fn buck_cmd(self) -> &'static str {
        match self {
            Self::One => "buck",
            Self::Two => "buck2",
        }
    }

    /// Antlir env variable to inform other tools which version of buck is being
    /// used
    pub fn antlir_env(self) -> (&'static str, &'static str) {
        ("ANTLIR_BUCK", self.buck_cmd())
    }
}

impl From<BuckVersion> for u8 {
    fn from(v: BuckVersion) -> u8 {
        match v {
            BuckVersion::One => 1,
            BuckVersion::Two => 2,
        }
    }
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
#[error("{0} is not a valid buck version")]
pub struct InvalidVersion(u8);

impl TryFrom<u8> for BuckVersion {
    type Error = InvalidVersion;

    fn try_from(v: u8) -> Result<BuckVersion, InvalidVersion> {
        match v {
            1 => Ok(BuckVersion::One),
            2 => Ok(BuckVersion::Two),
            x => Err(InvalidVersion(x)),
        }
    }
}

impl Serialize for BuckVersion {
    fn serialize<S>(&self, s: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        s.serialize_u8((*self).into())
    }
}

impl<'de> Deserialize<'de> for BuckVersion {
    fn deserialize<D>(d: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let v = u8::deserialize(d)?;
        v.try_into().map_err(D::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;

    #[test]
    fn cli_args() {
        #[derive(Debug, PartialEq, Eq, Parser)]
        struct Args {
            #[clap(value_enum)]
            buck_version: BuckVersion,
        }

        assert_eq!(
            Args {
                buck_version: BuckVersion::One
            },
            Args::try_parse_from(["argv0", "1"]).expect("valid args"),
        );
        assert_eq!(
            Args {
                buck_version: BuckVersion::Two
            },
            Args::try_parse_from(["argv0", "2"]).expect("valid args"),
        );
        // When buck5 comes out, this code will be (the least of) someone
        // else's problem(s)
        assert!(Args::try_parse_from(["argv0", "5"]).is_err());
    }

    #[test]
    fn serde() {
        assert_eq!(
            "1",
            serde_json::to_string(&BuckVersion::One).expect("infallible")
        );
        assert_eq!(
            "2",
            serde_json::to_string(&BuckVersion::Two).expect("infallible")
        );
        assert_eq!(
            BuckVersion::One,
            serde_json::from_str::<BuckVersion>("1").expect("valid buckversion")
        );
        assert_eq!(
            BuckVersion::Two,
            serde_json::from_str::<BuckVersion>("2").expect("valid buckversion")
        );
        assert!(serde_json::from_str::<BuckVersion>("5").is_err());
    }
}
