use anyhow::{Context, Result};
use clap::{Args, Parser};
use fbthrift::simplejson_protocol::deserialize;
use slog::Logger;

use metalos_host_configs::packages::generic::Package;
use package_download::{ensure_packages_on_disk_ignoring_artifacts, HttpsDownloader};

#[derive(Parser)]
pub enum Opts {
    /// Stage packages to the local machine
    Stage(Stage),
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
