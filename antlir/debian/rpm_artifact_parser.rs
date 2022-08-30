/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::io::Cursor;
use std::io::Read;
use std::path::Path;
use std::str::FromStr;

use anyhow::anyhow;
use anyhow::ensure;
use anyhow::Context;
use anyhow::Result;
use blob_store::get_blob;
use blob_store::get_sha2_hash;
use blob_store::Blob;
use blob_store::DownloadDetails;
use blob_store::StoreFormat;
use bytes::Buf;
use bytes::Bytes;
use flate2::bufread::GzDecoder;
use flate2::bufread::GzEncoder;
use flate2::Compression;
use quick_xml::de::from_str;
use quick_xml::events::BytesStart;
use quick_xml::events::BytesText;
use quick_xml::events::Event;
use quick_xml::se::Serializer;
use quick_xml::Reader;
use quick_xml::Writer;
use reqwest::Url;
use serde::Deserialize;
use serde::Serialize;
use slog::o;
use snapshotter_helpers::Architecture;

#[derive(Debug, PartialEq)]
pub enum Hash {
    Sha256([u8; 32]),
    Sha128([u8; 16]),
}

#[derive(Debug, PartialEq)]
pub struct Repo {
    pub repo_root_url: reqwest::Url,
    pub distro: String,
    pub arch: Architecture,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Default, Clone)]
pub struct Repomd {
    pub xmlns: String,
    #[serde(rename = "xmlns:rpm")]
    pub xmlns_rpm: String,
    pub revision: Text,
    pub data: Vec<FileData>,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Default, Clone)]
pub struct CheckSum {
    #[serde(rename = "type")]
    pub checksum_type: String,
    #[serde(rename = "$value")]
    pub hash: String,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Default, Clone)]
pub struct FileData {
    #[serde(rename = "type")]
    pub file_type: String,
    pub checksum: CheckSum,
    pub location: Location,
    pub timestamp: Text,
    pub size: Text,
    #[serde(rename = "open-size")]
    pub open_size: Text,
    #[serde(rename = "open-checksum")]
    pub open_checksum: CheckSum,
}
#[derive(Debug, Deserialize, Serialize, PartialEq, Default, Clone)]
pub struct Text {
    #[serde(rename = "$value")]
    pub value: String,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Default, Clone)]
pub struct Location {
    pub href: String,
}

#[derive(Debug, Deserialize, PartialEq, Default)]
pub struct PackageFile {
    #[serde(rename = "$value")]
    pub package_vec: Vec<Package>,
}

#[derive(Debug, Deserialize, PartialEq, Default)]
pub struct Package {
    pub name: String,
    pub arch: String,
    pub version: Version,
    pub checksum: CheckSum,
    pub location: Location,
}

#[derive(Debug, Deserialize, PartialEq, Default)]
pub struct Version {
    pub epoch: String,
    pub ver: String,
    pub rel: String,
}

#[derive(Debug, PartialEq)]
pub struct RpmPackage<'a> {
    pub name: String,
    pub arch: Architecture,
    pub version: String,
    pub checksum: Hash,
    pub location: String,
    pub repo: &'a Repo,
}

#[derive(Debug, PartialEq)]
pub struct RepoArtifact<'a> {
    pub file_type: String,
    pub checksum: Hash,
    pub location: String,
    pub repo: &'a Repo,
}
#[derive(Clone)]
pub struct UpdatedPrimaryPackage {
    pub content: Bytes,
    pub gzipped: Bytes,
    pub timestamp: String,
}

impl<'a> StoreFormat for &RpmPackage<'a> {
    fn store_format(&self) -> Result<DownloadDetails> {
        Ok(DownloadDetails {
            content: Blob::Url(self.repo.repo_root_url.join(&self.location)?),
            key: self.checksum.key(),
            name: self.name.clone(),
            version: self.version.clone(),
        })
    }
}
impl<'a> StoreFormat for &RepoArtifact<'a> {
    fn store_format(&self) -> Result<DownloadDetails> {
        Ok(DownloadDetails {
            content: Blob::Url(self.repo.repo_root_url.join(&self.location)?),
            key: self.checksum.key(),
            name: format!("{}:{}", self.file_type, self.repo.distro),
            version: format!("{:?}", self.checksum),
        })
    }
}

impl StoreFormat for UpdatedPrimaryPackage {
    fn store_format(&self) -> Result<DownloadDetails> {
        Ok(DownloadDetails {
            content: Blob::Blob(self.gzipped.clone()),
            key: format!("SHA256:{}", get_sha2_hash(&self.gzipped)),
            name: format!("Primary File: {}", get_sha2_hash(&self.gzipped)),
            version: get_sha2_hash(&self.gzipped),
        })
    }
}
impl StoreFormat for Repomd {
    fn store_format(&self) -> Result<DownloadDetails> {
        let serialized_data = self.serialized()?;
        Ok(DownloadDetails {
            content: Blob::Blob(serialized_data.clone()),
            key: format!("SHA256:{}", get_sha2_hash(&serialized_data)),
            name: format!("Primary File: {}", get_sha2_hash(&serialized_data)),
            version: get_sha2_hash(&serialized_data),
        })
    }
}

impl From<&UpdatedPrimaryPackage> for FileData {
    fn from(primary_art: &UpdatedPrimaryPackage) -> Self {
        let gzip_checksum = get_sha2_hash(&primary_art.gzipped);
        let open_checksum = get_sha2_hash(&primary_art.content);
        FileData {
            file_type: "primary".to_string(),
            checksum: CheckSum {
                checksum_type: "sha256".to_string(),
                hash: gzip_checksum.clone(),
            },
            location: Location {
                // Dnf does not allow any artificat url path
                // other than /repodata/<file_name>
                href: format!("/repodata/SHA256:{}-primary.xml.gz", gzip_checksum),
            },
            timestamp: Text {
                value: primary_art.timestamp.clone(),
            },
            size: Text {
                value: primary_art.gzipped.len().to_string(),
            },
            open_size: Text {
                value: primary_art.content.len().to_string(),
            },
            open_checksum: CheckSum {
                checksum_type: "sha256".to_string(),
                hash: open_checksum,
            },
        }
    }
}

impl Repomd {
    pub fn update(&self, primary_art: &UpdatedPrimaryPackage) -> Result<Self> {
        let updated_repomd: Repomd = Repomd {
            revision: self.revision.clone(),
            xmlns: self.xmlns.clone(),
            xmlns_rpm: self.xmlns_rpm.clone(),
            data: self
                .data
                .clone()
                .into_iter()
                .map(|file| {
                    if file.file_type == "primary" {
                        FileData::from(primary_art)
                    } else {
                        FileData {
                            file_type: file.file_type.clone(),
                            checksum: file.checksum.clone(),
                            location: Location {
                                href: format!(
                                    "/repodata/SHA256:{}-{}",
                                    file.checksum.hash,
                                    Path::new(&file.location.href)
                                        .file_name()
                                        .expect("expected to have file name in href")
                                        .to_str()
                                        .expect("href should contain a valid file name")
                                ),
                            },
                            timestamp: file.timestamp.clone(),
                            size: file.size.clone(),
                            open_size: file.open_size.clone(),
                            open_checksum: file.open_checksum,
                        }
                    }
                })
                .collect(),
        };
        Ok(updated_repomd)
    }
    pub fn serialized(&self) -> Result<Bytes> {
        let mut buffer = Vec::new();
        let writer = Writer::new_with_indent(&mut buffer, b' ', 2);
        let mut ser = Serializer::with_root(writer, Some("repomd"));

        self.serialize(&mut ser)?;
        let mut prelude: Vec<u8> = br#"<?xml version="1.0" encoding="UTF-8"?>"#.to_vec();
        prelude.extend(buffer);
        Ok(Bytes::from(prelude))
    }
}

impl UpdatedPrimaryPackage {
    pub fn new(package_data: String, timestamp: String) -> Result<Self> {
        let mut reader = Reader::from_str(&package_data);
        let mut writer = Writer::new(Cursor::new(Vec::new()));
        let mut buf = Vec::new();
        let mut checksum: Option<String> = None;
        loop {
            let event = reader.read_event(&mut buf)?;
            match &event {
                Event::Start(e) | Event::Empty(e) => {
                    if e.name() == b"package" {
                        checksum = None;
                    }
                    // if we come across a location tag, just dump it on the floor,
                    // we'll emit a fresh <location> element at the close of the
                    // location tag is an empty tag, we do not need to handle end event
                    // </package>
                    if e.name() == b"location" {
                        continue;
                    }
                    if e.name() == b"checksum" {
                        ensure!(checksum.is_none(), "found two <checksum> elements!");
                        let checksum_type = reader
                            .decode(
                                &e.attributes()
                                    .filter_map(Result::ok)
                                    .find(|a| a.key == b"type")
                                    .context("<checksum> missing \"type\"")?
                                    .value,
                            )
                            .context("checksum type not string")?
                            .to_owned();
                        let checksum_value = reader
                            .read_text(b"checksum", &mut Vec::new())
                            .context("while reading checksum value")?;
                        let mut elem = BytesStart::borrowed_name(b"checksum");
                        elem.push_attribute(("type", checksum_type.as_str()));
                        elem.push_attribute(("pkgid", "YES"));
                        writer.write_event(Event::Start(elem.to_borrowed()))?;
                        writer
                            .write_event(Event::Text(BytesText::from_plain_str(&checksum_value)))
                            .context("while writing out <checksum>")?;
                        writer.write_event(Event::End(elem.to_end()))?;
                        checksum = Some(format!(
                            "{}:{}",
                            checksum_type.to_uppercase(),
                            checksum_value
                        ));
                        continue;
                    }
                }
                Event::End(e) => {
                    if e.name() == b"package" {
                        let key = checksum
                            .take()
                            .context("reached </package> but never found <checksum>")?;

                        writer.write_event(Event::Empty(
                            BytesStart::borrowed_name(b"location")
                                .extend_attributes(vec![("href", format!("rpm/{}", key).as_str())])
                                .to_borrowed(),
                        ))?;
                    }
                }
                _ => (),
            }
            let eof = matches!(event, Event::Eof);
            writer.write_event(event)?;
            if eof {
                break;
            }
        }
        let plain_bytes = Bytes::from(writer.into_inner().into_inner());
        let mut gz = GzEncoder::new(plain_bytes.clone().reader(), Compression::fast());
        let mut buffer = Vec::new();
        gz.read_to_end(&mut buffer)?;
        Ok(UpdatedPrimaryPackage {
            content: plain_bytes,
            gzipped: Bytes::from(buffer),
            timestamp,
        })
    }
}

impl Hash {
    pub fn new(hash_type: String, hash: String) -> Result<Self> {
        match hash_type.to_lowercase().as_str() {
            "sha256" => Ok(Hash::Sha256(
                hex::decode(&hash)?
                    .try_into()
                    .map_err(|_| anyhow!("{} is not a valid sha256", hash))?,
            )),
            "sha128" => Ok(Hash::Sha128(
                hex::decode(&hash)?
                    .try_into()
                    .map_err(|_| anyhow!("{} is not a valid sha128", hash))?,
            )),
            _ => Err(anyhow!("unsupported hash")),
        }
    }
    pub fn key(&self) -> String {
        match *self {
            Hash::Sha256(sum) => format!("{}:{}", "SHA256", hex::encode(sum)),
            Hash::Sha128(sum) => format!("{}:{}", "SHA128", hex::encode(sum)),
        }
    }
}

impl<'a> RepoArtifact<'a> {
    pub fn new(repo: &'a Repo, repodata: &FileData) -> Result<Self> {
        Ok(RepoArtifact {
            file_type: repodata.file_type.clone(),
            checksum: Hash::new(
                repodata.checksum.checksum_type.clone(),
                repodata.checksum.hash.clone(),
            )?,
            location: repodata.location.href.clone(),
            repo,
        })
    }
    pub fn get_repo_artifacts<'b>(repo: &'b Repo, repomd: Repomd) -> Result<Vec<RepoArtifact<'b>>> {
        let rpm_artifacts: Result<Vec<RepoArtifact>> = repomd
            .data
            .iter()
            .map(|artifact| RepoArtifact::new(repo, artifact))
            .collect();
        rpm_artifacts
    }
    pub async fn get_repmod(
        repo: &Repo,
        client: &reqwest::Client,
        logger: slog::Logger,
    ) -> Result<Repomd> {
        let release_text = String::from_utf8(
            get_blob(
                repo.repo_root_url
                    .join("repodata/repomd.xml")
                    .context("failed to join")?,
                client,
                4,
                logger.new(o!("file" =>"Repomd")),
            )
            .await?
            .to_vec(),
        )?;
        let repomd: Repomd = from_str(&release_text).context("failed to deserialize repomd.xml")?;
        Ok(repomd)
    }
}

impl<'a> RpmPackage<'a> {
    pub fn new(repo: &'a Repo, rpm_data: Package) -> Result<Self> {
        Ok(RpmPackage {
            name: rpm_data.name,
            arch: Architecture::from_str(&rpm_data.arch)?,
            version: format!(
                "{}-{}-{}",
                rpm_data.version.epoch, rpm_data.version.ver, rpm_data.version.rel
            ),
            checksum: Hash::new(rpm_data.checksum.checksum_type, rpm_data.checksum.hash)?,
            location: rpm_data.location.href,
            repo,
        })
    }
    pub async fn get_primary_artifact(
        artifact_url: reqwest::Url,
        client: &reqwest::Client,
        logger: slog::Logger,
    ) -> Result<String> {
        if !artifact_url.as_str().ends_with(".gz") {
            return Err(anyhow!(
                "gzip is the only supported encoding in primary artifact"
            ));
        }
        let blob = get_blob(artifact_url, client, 4, logger.new(o!("file" =>"Primary")))
            .await?
            .to_vec();
        let mut gz = GzDecoder::new(&blob[..]);
        let mut package_data = String::new();
        gz.read_to_string(&mut package_data)?;
        Ok(package_data)
    }
    pub fn get_rpm_packages<'b>(
        repo: &'b Repo,
        package_data: String,
    ) -> Result<Vec<RpmPackage<'b>>> {
        let package_file: PackageFile =
            from_str(&package_data).context("failed to deserialize package file")?;
        let rpm_packages: Result<Vec<RpmPackage<'b>>> = package_file
            .package_vec
            .into_iter()
            .map(|pkg| RpmPackage::new(repo, pkg))
            .collect();
        rpm_packages
    }
}

impl Repo {
    pub fn new(repo_url: &str, distro: String, arch: String) -> Result<Repo> {
        Ok(Repo {
            repo_root_url: Url::parse(repo_url)
                .with_context(|| format!("invalid url {}", repo_url))?,
            distro,
            arch: Architecture::from_str(arch.as_str())?,
        })
    }
}

#[cfg(test)]
mod test {
    use super::*;
    #[test]
    fn test_repomd_file_parse() {
        let release_file = include_str!("test_data/repomd_small.xml");
        let repo_md: Repomd = from_str(release_file).unwrap();
        let expected_repomd = Repomd {
            xmlns: "http://linux.duke.edu/metadata/repo".to_string(),
            xmlns_rpm: "http://linux.duke.edu/metadata/rpm".to_string(),
            revision: Text {
                value: "1660187336".to_string(),
            },
            data: vec![FileData {
                file_type: "primary".to_string(),
                checksum: CheckSum {
                    checksum_type: "sha256".to_string(),
                    hash: "b0ba64e97f7ed7099845b2fafbd58f6a03f047f8dd009b4d779a242130ad42ec".to_string(),
                },
                location: Location {
                    href: "repodata/b0ba64e97f7ed7099845b2fafbd58f6a03f047f8dd009b4d779a242130ad42ec-primary.xml.gz".to_string(),
                },
                timestamp: Text {
                    value: "1660187336".to_string(),
                },
                open_checksum: CheckSum {
                    checksum_type: "sha256".to_string(),
                    hash: "99447c570c18f8219957bff74fb99a5f11006beb9a2e17b9dcb7bb48eef6b316".to_string(),
                },
                size: Text { value: "35546".to_string() },
                open_size: Text { value: "278882".to_string() },
            }],
        };
        assert_eq!(repo_md, expected_repomd);
    }
    #[test]
    fn test_repomd_rewrite() {
        let repomd_file = include_str!("test_data/repomd_full.xml");
        let updated_repomd_str = include_str!("test_data/repomd_rewrite.xml");
        let repomd: Repomd = from_str(repomd_file).unwrap();
        let expected_repomd: Repomd = from_str(updated_repomd_str).unwrap();
        let updated_repomd_file = repomd
            .update(&UpdatedPrimaryPackage {
                content: Bytes::new(),
                gzipped: Bytes::new(),
                timestamp: "1660187336".to_string(),
            })
            .unwrap();
        let returned_repomd_str =
            String::from_utf8(updated_repomd_file.serialized().unwrap().to_vec()).unwrap();
        let returned_repomd: Repomd = from_str(&returned_repomd_str).unwrap();
        assert_eq!(returned_repomd, expected_repomd);
        assert_eq!(
            returned_repomd_str, updated_repomd_str,
            "byte to byte comparision might fail when updating rewrite logic"
        );
    }
    #[test]
    fn test_package_file_rewrite() {
        let repo = Repo {
            repo_root_url: Url::parse("http://test").unwrap(),
            distro: "test".to_string(),
            arch: Architecture::Unknown("test".to_string()),
        };
        let primary_str = include_str!("test_data/primary.xml");
        let primary_rewrite_str = include_str!("test_data/primary_rewrite.xml");
        let updated_primary =
            UpdatedPrimaryPackage::new(primary_str.to_string(), "1660187336".to_string()).unwrap();
        let updated_primary_str = String::from_utf8(updated_primary.content.to_vec()).unwrap();
        let packages = RpmPackage::get_rpm_packages(&repo, updated_primary_str.clone()).unwrap();
        let expected_packages =
            RpmPackage::get_rpm_packages(&repo, primary_rewrite_str.to_string()).unwrap();
        assert_eq!(packages, expected_packages);
        assert_eq!(
            primary_rewrite_str, updated_primary_str,
            "byte to byte comparision might fail when updating rewrite logic"
        )
    }
}
