/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::str::FromStr;
use std::time::Duration;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Result;
use blob_store::get_blob;
use clap::Parser as ClapParser;
use fbinit::FacebookInit;
use quick_xml::de::from_str;
use reqwest::Url;
use serde::Deserialize;
use slog::info;
use slog::o;
use slog::Drain;
use snapshotter_helpers::Architecture;
use snapshotter_helpers::Args;

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
        let package_data = String::from_utf8(
            get_blob(artifact_url, client, 4, logger.new(o!("file" =>"Primary")))
                .await?
                .to_vec(),
        )?;
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

async fn snapshot(repo: Repo, logger: slog::Logger) -> Result<()> {
    let client = reqwest::ClientBuilder::new()
        .redirect(reqwest::redirect::Policy::limited(3))
        .pool_idle_timeout(None)
        .tcp_keepalive(Some(Duration::from_secs(3600)))
        .build()?;
    let repo_artifacts =
        RepoArtifact::get_repo_artifacts(&repo, &client, logger.new(o!("file" => "repomd")))
            .await?;
    let mut primary_repo_artifact: Option<RepoArtifact> = None;
    for repo_artifact in repo_artifacts {
        if repo_artifact.file_type.eq("primary") {
            primary_repo_artifact = Some(repo_artifact)
        }
    }
    let rpm_packages = RpmPackage::get_rpm_packages(
        repo.repo_root_url.join(
            primary_repo_artifact
                .context("primary file not found")?
                .location
                .as_str(),
        )?,
        &repo,
        &client,
        logger.new(o!("file" => "rpms")),
    )
    .await?;
    println!("{:?}", rpm_packages);
    Ok(())
}

#[fbinit::main]
async fn main(_fb: FacebookInit) -> Result<()> {
    let args = Args::parse();
    let repo = Repo::new(&args.repourl, args.distro, args.arch)?;
    let decorator = slog_term::TermDecorator::new().build();
    let drain = slog_term::FullFormat::new(decorator).build().fuse();
    let drain = slog_async::Async::new(drain).build().fuse();

    let log = slog::Logger::root(drain, o!());
    let release_hash = snapshot(repo, log.new(o!("repo" => "test"))).await?;
    info!(log, ""; "release_hash" => release_hash);
    Ok(())
}
