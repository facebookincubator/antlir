/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::io::BufRead;
use std::io::BufReader;
use std::io::BufWriter;
use std::io::Write;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use clap::Parser;
use serde::Deserialize;
use serde::Serialize;
use serde_with::serde_as;
use snapshot_common::Checksums;

use super::Location;
use super::XmlConv;

#[derive(Parser, Debug)]
pub(crate) struct ParseRepomd {
    /// Path to repomd.xml
    repomd_xml: PathBuf,
    /// Output path (or - for stdout)
    out: PathBuf,
}

#[derive(Debug, Deserialize)]
struct Repomd {
    data: Vec<RepomdData>,
}

#[serde_as]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct RepomdData {
    #[serde(rename = "@type")]
    typ: String,
    #[serde_as(as = "XmlConv")]
    #[serde(rename = "checksum")]
    checksums: Checksums,
    location: Location,
}

#[derive(Debug, PartialEq, Serialize)]
struct Out {
    primary: BlobDl,
    filelists: BlobDl,
    other: BlobDl,
}

#[derive(Debug, PartialEq, Serialize)]
struct BlobDl {
    href: String,
    checksums: Checksums,
    filetype: String,
}

fn get_blob_dl(repomd: &Repomd, typ: &str) -> Result<BlobDl> {
    let d = repomd
        .data
        .iter()
        .find(|d| d.typ == typ)
        .with_context(|| format!("no data of type '{typ}'"))?;
    Ok(BlobDl {
        href: d.location.href.to_owned(),
        checksums: d.checksums.clone(),
        filetype: d
            .location
            .href
            .rsplit_once(".")
            .context("location had no filetype suffix")?
            .1
            .to_owned(),
    })
}

fn parse<R: BufRead>(reader: R) -> Result<Out> {
    let repomd: Repomd = quick_xml::de::from_reader(reader).context("while reading xml")?;
    Ok(Out {
        primary: get_blob_dl(&repomd, "primary")?,
        filelists: get_blob_dl(&repomd, "filelists")?,
        other: get_blob_dl(&repomd, "other")?,
    })
}

impl ParseRepomd {
    #[tracing::instrument(ret, err)]
    pub(crate) fn run(&self) -> Result<()> {
        let infile = BufReader::new(stdio_path::open(&self.repomd_xml)?);
        let out = parse(infile)
            .with_context(|| format!("while parsing {}", self.repomd_xml.display()))?;

        let mut outfile = BufWriter::new(stdio_path::create(&self.out)?);
        serde_json::to_writer_pretty(&mut outfile, &out)?;
        outfile.write_all(b"\n")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    #[test]
    fn test_parse() {
        assert_eq!(
            Out {
                primary: BlobDl {
                    href: "repodata/667884884bc57fb9652ec407549d3463e25e36ed145cb2de19485e550789ba95-primary.xml.gz".into(),
                    checksums: Checksums::new_sha256("667884884bc57fb9652ec407549d3463e25e36ed145cb2de19485e550789ba95".into()),
                    filetype: "gz".into(),
                },
                filelists: BlobDl {
                    href: "repodata/f65070f2741942596762ce01084d000b2b340ffe0a30741857ad895658a5ef8a-filelists.xml.gz".into(),
                    checksums: Checksums::new_sha256("f65070f2741942596762ce01084d000b2b340ffe0a30741857ad895658a5ef8a".into()),
                    filetype: "gz".into(),
                },
                other: BlobDl {
                    href: "repodata/6904eff5506edbcd3fb2459c08c2d52cc6707cff31000df4226aebf8989cdc7c-other.xml.gz".into(),
                    checksums: Checksums::new_sha256("6904eff5506edbcd3fb2459c08c2d52cc6707cff31000df4226aebf8989cdc7c".into()),
                    filetype: "gz".into(),
                },
            },
            parse(Cursor::new(include_str!("../../../../testdata/yum/repomd.xml")))
                .expect("failed to parse test repomd.xml")
        );
    }
}
