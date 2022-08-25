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
use std::time::Duration;

use anyhow::anyhow;
use anyhow::ensure;
use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use blob_store::get_blob;
use blob_store::get_sha2_hash;
use blob_store::upload;
use blob_store::Blob;
use blob_store::DownloadDetails;
use blob_store::PackageBackend;
use blob_store::RateLimitedPackageBackend;
use blob_store::StoreFormat;
use bytes::Buf;
use bytes::Bytes;
use clap::Parser;
use fbinit::FacebookInit;
use flate2::bufread::GzDecoder;
use flate2::bufread::GzEncoder;
use flate2::Compression;
use futures::future::try_join_all;
use governor::clock::DefaultClock;
use governor::state::InMemoryState;
use governor::state::NotKeyed;
use manifold_client::cpp_client::ClientOptionsBuilder;
use manifold_client::cpp_client::ManifoldCppClient;
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
use slog::info;
use slog::o;
use slog::Drain;
use snapshotter_helpers::Architecture;
use snapshotter_helpers::Args;
use snapshotter_helpers::ANTLIR_SNAPSHOTS_BUCKET;
use snapshotter_helpers::API_KEY;

#[derive(Debug, PartialEq)]
enum Hash {
    Sha256([u8; 32]),
    Sha128([u8; 16]),
}

#[derive(Debug, PartialEq)]
struct Repo {
    repo_root_url: reqwest::Url,
    distro: String,
    arch: Architecture,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Default, Clone)]
struct Repomd {
    xmlns: String,
    #[serde(rename = "xmlns:rpm")]
    xmlns_rpm: String,
    revision: Text,
    data: Vec<FileData>,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Default, Clone)]
struct CheckSum {
    #[serde(rename = "type")]
    checksum_type: String,
    #[serde(rename = "$value")]
    hash: String,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Default, Clone)]
struct FileData {
    #[serde(rename = "type")]
    file_type: String,
    checksum: CheckSum,
    location: Location,
    timestamp: Text,
    size: Text,
    #[serde(rename = "open-size")]
    open_size: Text,
    #[serde(rename = "open-checksum")]
    open_checksum: CheckSum,
}
#[derive(Debug, Deserialize, Serialize, PartialEq, Default, Clone)]
struct Text {
    #[serde(rename = "$value")]
    value: String,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Default, Clone)]
struct Location {
    href: String,
}

#[derive(Debug, Deserialize, PartialEq, Default)]
struct PackageFile {
    #[serde(rename = "$value")]
    package_vec: Vec<Package>,
}

#[derive(Debug, Deserialize, PartialEq, Default)]
struct Package {
    name: String,
    arch: String,
    version: Version,
    checksum: CheckSum,
    location: Location,
}

#[derive(Debug, Deserialize, PartialEq, Default)]
struct Version {
    epoch: String,
    ver: String,
    rel: String,
}

#[derive(Debug, PartialEq)]
struct RpmPackage<'a> {
    name: String,
    arch: Architecture,
    version: String,
    checksum: Hash,
    location: String,
    repo: &'a Repo,
}

#[derive(Debug, PartialEq)]
struct RepoArtifact<'a> {
    file_type: String,
    checksum: Hash,
    location: String,
    repo: &'a Repo,
}
#[derive(Clone)]
struct UpdatedPrimaryPackage {
    content: Bytes,
    gzipped: Bytes,
    timestamp: String,
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
    fn update(&self, primary_art: &UpdatedPrimaryPackage) -> Result<Self> {
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
    fn serialized(&self) -> Result<Bytes> {
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
    fn new(package_data: String, timestamp: String) -> Result<Self> {
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
    fn new(hash_type: String, hash: String) -> Result<Self> {
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
    fn key(&self) -> String {
        match *self {
            Hash::Sha256(sum) => format!("{}:{}", "SHA256", hex::encode(sum)),
            Hash::Sha128(sum) => format!("{}:{}", "SHA128", hex::encode(sum)),
        }
    }
}

impl<'a> RepoArtifact<'a> {
    fn new(repo: &'a Repo, repodata: &FileData) -> Result<Self> {
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
    fn get_repo_artifacts<'b>(repo: &'b Repo, repomd: Repomd) -> Result<Vec<RepoArtifact<'b>>> {
        let rpm_artifacts: Result<Vec<RepoArtifact>> = repomd
            .data
            .iter()
            .map(|artifact| RepoArtifact::new(repo, artifact))
            .collect();
        rpm_artifacts
    }
    async fn get_repmod(
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
    fn new(repo: &'a Repo, rpm_data: Package) -> Result<Self> {
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
    async fn get_primary_artifact(
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
    fn get_rpm_packages<'b>(repo: &'b Repo, package_data: String) -> Result<Vec<RpmPackage<'b>>> {
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
    fn new(repo_url: &str, distro: String, arch: String) -> Result<Repo> {
        Ok(Repo {
            repo_root_url: Url::parse(repo_url)
                .with_context(|| format!("invalid url {}", repo_url))?,
            distro,
            arch: Architecture::from_str(arch.as_str())?,
        })
    }
}

async fn snapshot(
    repo: Repo,
    storage_backend: impl PackageBackend,
    read_rl: Option<governor::RateLimiter<NotKeyed, InMemoryState, DefaultClock>>,
    write_rl: Option<governor::RateLimiter<NotKeyed, InMemoryState, DefaultClock>>,
    write_throughput: Option<governor::RateLimiter<NotKeyed, InMemoryState, DefaultClock>>,
    logger: slog::Logger,
) -> Result<String> {
    let client = reqwest::ClientBuilder::new()
        .redirect(reqwest::redirect::Policy::limited(3))
        .pool_idle_timeout(None)
        .tcp_keepalive(Some(Duration::from_secs(3600)))
        .no_gzip()
        .build()?;
    let rl_packagebackend =
        RateLimitedPackageBackend::new(read_rl, write_rl, write_throughput, storage_backend);
    let repomd =
        RepoArtifact::get_repmod(&repo, &client, logger.new(o!("file" => "repomd"))).await?;
    let repo_artifacts = RepoArtifact::get_repo_artifacts(&repo, repomd.clone())?;

    let mut primary_repo_url: Option<reqwest::Url> = None;
    let mut timestamp: Option<String> = None;
    for repo_artifact in &repomd.data {
        if repo_artifact.file_type.eq("primary") {
            primary_repo_url = Some(repo.repo_root_url.join(&repo_artifact.location.href)?);
            timestamp = Some(repo_artifact.timestamp.value.clone());
        }
    }
    let primary_artifact = RpmPackage::get_primary_artifact(
        primary_repo_url.clone().context("primary not found")?,
        &client,
        logger.new(o!("file" => "primary")),
    )
    .await?;
    let rpm_packages = RpmPackage::get_rpm_packages(&repo, primary_artifact.clone())?;

    let updated_primary_repo_artifact =
        UpdatedPrimaryPackage::new(primary_artifact, timestamp.context("primary not found")?)?;

    let updated_repomd = repomd.update(&updated_primary_repo_artifact)?;

    let release_hash = get_sha2_hash(updated_repomd.serialized()?);

    try_join_all(rpm_packages.iter().map(|rpm| {
        upload(
            rpm,
            &rl_packagebackend,
            &client,
            logger.new(o!("package" => rpm.name.clone())),
        )
    }))
    .await?;
    try_join_all(
        repo_artifacts
            .iter()
            .filter(|artifact| !artifact.file_type.eq("primary"))
            .map(|repo_artifact| {
                upload(
                    repo_artifact,
                    &rl_packagebackend,
                    &client,
                    logger.new(o!("repo_artifact_file" => repo_artifact.file_type.clone())),
                )
            }),
    )
    .await?;
    upload(
        updated_primary_repo_artifact,
        &rl_packagebackend,
        &client,
        logger.new(o!("repo_artifact_file" => "updated_primary_file")),
    )
    .await?;
    upload(
        updated_repomd,
        &rl_packagebackend,
        &client,
        logger.new(o!("repo_artifact_file" => "updated_repomd")),
    )
    .await?;
    Ok(release_hash)
}

#[fbinit::main]
async fn main(fb: FacebookInit) -> Result<()> {
    let args = Args::parse();
    let repo = Repo::new(&args.repourl, args.distro.clone(), args.arch.clone())?;
    let manifold_client_opts = ClientOptionsBuilder::default()
        .api_key(API_KEY)
        .build()
        .map_err(Error::msg)?;
    let manifold_client =
        ManifoldCppClient::from_options(fb, ANTLIR_SNAPSHOTS_BUCKET, &manifold_client_opts)
            .map_err(Error::from)?;
    let decorator = slog_term::TermDecorator::new().build();
    let drain = slog_term::FullFormat::new(decorator).build().fuse();
    let drain = slog_async::Async::new(drain).build().fuse();

    let log = slog::Logger::root(drain, o!());
    let release_hash = snapshot(
        repo,
        manifold_client,
        args.readqps,
        args.writeqps,
        args.writethroughput,
        log.new(o!("repo" => args.distro, "arch" => args.arch )),
    )
    .await?;
    info!(log, ""; "release_hash" => release_hash);
    Ok(())
}
