use anyhow::Context;
use anyhow::Result;
use clap::Args;
use clap::Parser;
use fbthrift::simplejson_protocol::deserialize;
use futures::TryStreamExt;
use slog::Logger;

use metalos_host_configs::packages::generic::Package;
use package_download::ensure_packages_on_disk_ignoring_artifacts;
use package_download::staged_packages;
use package_download::HttpsDownloader;

#[derive(Parser)]
pub enum Opts {
    /// Stage packages to the local machine
    Stage(Stage),
    /// List the locally staged packages
    List,
}

#[derive(Args)]
pub struct Stage {
    /// JSONified package thrift struct
    pkg_configs: Vec<String>,
}

/// Stage packages in an ad-hoc fashion without any supporting host_config
pub async fn stage_packages(log: Logger, stage: Stage) -> Result<()> {
    let downloader = HttpsDownloader::new().context("while constructing HTTPS downloader")?;
    let packages: Vec<Package> = stage
        .pkg_configs
        .into_iter()
        .map(|conf_str| deserialize(conf_str.as_bytes()).context("while deserializing json"))
        .collect::<Result<_>>()?;
    ensure_packages_on_disk_ignoring_artifacts(log, &downloader, &packages)
        .await
        .context("while downloading packages")
}

/// List all packages that have been staged locally
pub async fn list(_log: Logger) -> Result<()> {
    let staged: Vec<Package> = staged_packages().await?.try_collect().await?;
    // TODO: pretty-output a JSON-ified list, rather than JSON-ified items
    for pkg in staged {
        println!("{pkg:?}");
    }
    Ok(())
}
