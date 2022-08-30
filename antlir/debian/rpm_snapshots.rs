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
use rpm_artifact_parser::Repo;
use rpm_artifact_parser::RepoArtifact;
use rpm_artifact_parser::RpmPackage;
use rpm_artifact_parser::UpdatedPrimaryPackage;
use serde::Deserialize;
use serde::Serialize;
use slog::info;
use slog::o;
use slog::Drain;
use snapshotter_helpers::Architecture;
use snapshotter_helpers::Args;
use snapshotter_helpers::ANTLIR_SNAPSHOTS_BUCKET;
use snapshotter_helpers::API_KEY;

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
        .connect_timeout(Duration::from_secs(3))
        .tcp_keepalive(Some(Duration::from_secs(7200)))
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
