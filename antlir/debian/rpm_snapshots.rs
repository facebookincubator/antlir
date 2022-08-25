/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::io::Cursor;
use std::io::Read;
use std::str::FromStr;
use std::time::Duration;

use anyhow::anyhow;
use anyhow::ensure;
use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use blob_store::get_blob;
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
use quick_xml::Reader;
use quick_xml::Writer;
use reqwest::Url;
use serde::Deserialize;
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

#[derive(Debug, Deserialize, PartialEq, Default)]
struct Repomd {
    data: Vec<FileData>,
}

#[derive(Debug, Deserialize, PartialEq, Default)]
struct CheckSum {
    #[serde(rename = "type")]
    checksum_type: String,
    #[serde(rename = "$value")]
    hash: String,
}

#[derive(Debug, Deserialize, PartialEq, Default)]
struct FileData {
    #[serde(rename = "type")]
    file_type: String,
    checksum: CheckSum,
    location: Location,
}

#[derive(Debug, Deserialize, PartialEq, Default)]
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

struct UpdatedPrimaryPackage {
    content: Bytes,
    gzipped: Bytes,
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

impl UpdatedPrimaryPackage {
    async fn new(
        primary_package_url: reqwest::Url,
        client: &reqwest::Client,
        logger: slog::Logger,
    ) -> Result<Self> {
        if !primary_package_url.as_str().ends_with(".gz") {
            return Err(anyhow!(
                "gzip is the only supported encoding in primary artifact"
            ));
        }
        println!("here");
        let blob = get_blob(
            primary_package_url,
            client,
            4,
            logger.new(o!("file" =>"Primary")),
        )
        .await?
        .to_vec();
        let mut gz = GzDecoder::new(&blob[..]);
        let mut package_data = String::new();
        gz.read_to_string(&mut package_data)?;
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
    async fn get_repo_artifacts<'b>(
        repo: &'b Repo,
        client: &'b reqwest::Client,
        logger: slog::Logger,
    ) -> Result<Vec<RepoArtifact<'b>>> {
        let repomd = RepoArtifact::get_repmod(repo, client, logger).await?;
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
    async fn get_rpm_packages<'b>(
        artifact_url: reqwest::Url,
        repo: &'b Repo,
        client: &'b reqwest::Client,
        logger: slog::Logger,
    ) -> Result<Vec<RpmPackage<'b>>> {
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
) -> Result<()> {
    let client = reqwest::ClientBuilder::new()
        .redirect(reqwest::redirect::Policy::limited(3))
        .pool_idle_timeout(None)
        .tcp_keepalive(Some(Duration::from_secs(3600)))
        .no_gzip()
        .build()?;
    let rl_packagebackend =
        RateLimitedPackageBackend::new(read_rl, write_rl, write_throughput, storage_backend);
    let repo_artifacts =
        RepoArtifact::get_repo_artifacts(&repo, &client, logger.new(o!("file" => "repomd")))
            .await?;
    let mut primary_repo_url: Option<reqwest::Url> = None;
    for repo_artifact in &repo_artifacts {
        if repo_artifact.file_type.eq("primary") {
            primary_repo_url = Some(repo.repo_root_url.join(repo_artifact.location.as_str())?)
        }
    }
    let rpm_packages = RpmPackage::get_rpm_packages(
        primary_repo_url.clone().context("primary file not found")?,
        &repo,
        &client,
        logger.new(o!("file" => "rpms")),
    )
    .await?;
    let _update_primary_rpm_artifact = UpdatedPrimaryPackage::new(
        primary_repo_url.context("primary file not found")?.clone(),
        &client,
        logger.new(o!("update" => "primary")),
    )
    .await?;

    try_join_all(rpm_packages.iter().map(|rpm| {
        upload(
            rpm,
            &rl_packagebackend,
            &client,
            logger.new(o!("package" => rpm.name.clone())),
        )
    }))
    .await?;
    try_join_all(repo_artifacts.iter().map(|repo_artifact| {
        upload(
            repo_artifact,
            &rl_packagebackend,
            &client,
            logger.new(o!("repo_artifact_file" => repo_artifact.file_type.clone())),
        )
    }))
    .await?;
    Ok(())
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
