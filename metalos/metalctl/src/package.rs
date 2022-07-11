use anyhow::Context;
use anyhow::Result;
use clap::Args;
use clap::Parser;
use fbthrift::simplejson_protocol::deserialize;
use futures::TryStreamExt;
use slog::Logger;
use std::io::Read;

use metalos_host_configs::packages::generic::Package;
use metalos_host_configs::packages::generic::Packages;
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
    /// JSONified packages thrift struct
    pkg_configs: Option<String>,
}

/// Stage packages in an ad-hoc fashion without any supporting host_config
pub async fn stage_packages(log: Logger, stage: Stage) -> Result<()> {
    let pkgs: Packages = match stage.pkg_configs {
        Some(cfg) => deserialize(cfg.as_bytes())?,
        //None => panic!("foo"),
        None => {
            let mut stdin = std::io::stdin();
            let mut buf = Vec::new();
            stdin.read_to_end(&mut buf)?;
            deserialize(&buf)?
        }
    };
    let downloader = HttpsDownloader::new().context("while constructing HTTPS downloader")?;
    ensure_packages_on_disk_ignoring_artifacts(log, &downloader, &pkgs.packages)
        .await
        .context("while downloading packages")
}

/// List all packages that have been staged locally with JSON
pub async fn list(_log: Logger) -> Result<()> {
    let staged: Vec<Package> = staged_packages().await?.try_collect().await?;
    println!("{}", Packages { packages: staged });
    Ok(())
}
