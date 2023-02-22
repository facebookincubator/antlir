/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeSet;
use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::process::Command;
use std::process::Stdio;

use anyhow::Context;
use anyhow::Error;
use features::rpms::Rpm2;
use features::rpms::Rpm2Item;
use http::Uri;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Deserializer;
use serde_with::serde_as;
use serde_with::DisplayFromStr;
use tracing::trace;

use crate::CompileFeature;
use crate::CompilerContext;
use crate::Result;

mod dnf_proxy;

#[serde_as]
#[derive(Debug, Serialize)]
struct DriverSpec<'a> {
    #[serde_as(as = "HashMap<_, &[DisplayFromStr]>")]
    repos: HashMap<&'a str, &'a [Uri]>,
    items: &'a [Rpm2Item<'a>],
    install_root: &'a Path,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum DownloadStatus {
    Ok,
    Err(String),
    AlreadyExists,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum TransactionOperation {
    /// Package cleanup is being performed.
    Cleanup,
    /// Package is being installed as a downgrade
    Downgrade,
    /// Installed package is being downgraded
    Downgraded,
    /// Package is being installed
    Install,
    /// Package is obsoleting another package
    Obsolete,
    /// Installed package is being obsoleted
    Obsoleted,
    /// Package is installed as a reinstall
    Reinstall,
    /// Installed package is being reinstalled
    Reinstalled,
    /// Package is being removed
    Remove,
    /// Package is installed as an upgrade
    Upgrade,
    /// Installed package is being upgraded
    Upgraded,
    /// Package is being verified
    Verify,
    /// Package scriptlet is being performed
    Scriptlet,
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
struct Package {
    name: String,
    evr: String,
    arch: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
#[allow(dead_code)] // I want to log structured data
enum DriverEvent {
    DownloadStarted {
        total_files: usize,
        total_bytes: usize,
    },
    PackageDownloaded {
        package: Package,
        status: DownloadStatus,
    },
    TransactionResolved {
        install: BTreeSet<Package>,
        remove: BTreeSet<Package>,
    },
    TxItem {
        package: Package,
        operation: TransactionOperation,
    },
    TxError(String),
}

/// Relatively simple implementation of rpm features. This does not yet respect
/// version locks.
impl<'a> CompileFeature for Rpm2<'a> {
    #[tracing::instrument(name = "rpms", skip(self, ctx), ret, err)]
    fn compile(&self, ctx: &CompilerContext) -> Result<()> {
        let proxy = dnf_proxy::DnfProxy::start(ctx).context("while setting up dnf proxy")?;

        let input = serde_json::to_string(&DriverSpec {
            repos: proxy
                .dnf_conf()
                .repos()
                .iter()
                .map(|(id, c)| (id.as_str(), c.base_urls()))
                .collect(),
            items: &self.items,
            install_root: ctx.root(),
        })
        .context("while serializing dnf-driver input")?;

        {
            let mut f = OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .mode(0o555)
                .open("/antlir/dnf-driver.py")
                .context("while opening /antlir/dnf-driver.py")?;
            f.write_all(include_bytes!("../../dnf-driver.py"))
                .context("while writing out /antlir/dnf-driver.py")?;
        }

        let mut child = Command::new("/antlir/dnf-driver.py")
            .arg(&input)
            .stdout(Stdio::piped())
            .spawn()
            .context("while spawning dnf-driver.py")?;

        let deser = Deserializer::from_reader(child.stdout.take().expect("this is a pipe"));
        for event in deser.into_iter::<DriverEvent>() {
            let event = event.context("while deserializing even from dnf-driver.py")?;
            trace!("dnf-driver: {event:?}");
        }

        let result = child.wait().context("while waiting for dnf-driver.py")?;
        if !result.success() {
            return Err(Error::msg("dnf-driver.py failed").into());
        }
        Ok(())
    }
}
