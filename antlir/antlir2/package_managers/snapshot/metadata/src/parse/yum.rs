/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use anyhow::Result;
use clap::Parser;
use clap::Subcommand;
use serde::Deserialize;
use serde::Serialize;
use serde::de::Error as _;
use serde_with::DeserializeAs;
use snapshot_common::Checksums;

mod primary;
mod repomd;

#[derive(Debug, Parser)]
pub(crate) struct Parse {
    #[clap(subcommand)]
    sub: Sub,
}

#[derive(Debug, Subcommand)]
enum Sub {
    /// Parse a repomd.xml file and return information required to download the
    /// 'primary', 'filelists' and 'other' documents.
    Repomd(repomd::ParseRepomd),
    /// Parse a primary.xml file and return information required to identify and
    /// download RPMs from the repo
    Primary(primary::ParsePrimary),
}

impl Parse {
    pub(crate) fn run(&self) -> Result<()> {
        match &self.sub {
            Sub::Repomd(sub) => sub.run(),
            Sub::Primary(sub) => sub.run(),
        }
    }
}

fn checksums_from_xml<E: serde::de::Error>(ty: &str, text: String) -> Result<Checksums, E> {
    match ty {
        "sha1" => Checksums::new_sha1_hex(text).map_err(E::custom),
        "sha256" => Checksums::new_sha256_hex(text).map_err(E::custom),
        _ => Err(E::custom(format!("unknown checksum type: {}", ty))),
    }
}

struct XmlConv;

impl<'de> DeserializeAs<'de, Checksums> for XmlConv {
    fn deserialize_as<D>(deserializer: D) -> Result<Checksums, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Debug, Deserialize)]
        struct Xml {
            #[serde(rename = "@type")]
            ty: String,
            #[serde(rename = "$value")]
            text: String,
        }
        let x = Xml::deserialize(deserializer).map_err(D::Error::custom)?;
        checksums_from_xml(&x.ty, x.text)
    }
}

#[derive(Debug, PartialEq, Deserialize, Serialize)]
struct Location {
    #[serde(alias = "@href")]
    href: String,
}
