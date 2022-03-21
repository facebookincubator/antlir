/*
 * Copyright (c) Meta Platforms, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use anyhow::{Context, Result};
use structopt::StructOpt;

use service::ServiceInstance;
use state::Token;

mod volumes;
use volumes::ServiceVolumes;

#[derive(StructOpt)]
enum Operation {
    /// Prepare a service's subvolumes for execution. Must be invoked
    /// immediately before the service starts, via
    /// metalos-native-service@.service
    Init(SvcOpts),
    /// Cleanup subvolumes after a service execution. Must be invoked some time
    /// after the native service stops, via metalos-native-service@.service
    Deinit(SvcOpts),
}

impl Operation {
    fn token(&self) -> Token<ServiceInstance> {
        match self {
            Self::Init(o) => o.token,
            Self::Deinit(o) => o.token,
        }
    }
}

#[derive(Debug, StructOpt)]
struct SvcOpts {
    /// Token pointing to a serialized ServiceInstance. Generated automatically
    /// as part of the native service lifecycle transitions.
    #[structopt(long)]
    token: Token<ServiceInstance>,
}

fn init(svc: ServiceInstance) -> Result<()> {
    ServiceVolumes::create(&svc).context("while creating service subvolumes")?;
    Ok(())
}

fn deinit(svc: ServiceInstance) -> Result<()> {
    let vols = ServiceVolumes::get(&svc).context("while getting service subvolumes")?;
    vols.delete()
        .context("while deleting ephemeral subvolumes")?;
    Ok(())
}

fn main() -> Result<()> {
    let op = Operation::from_args();
    let svc = state::load(op.token())
        .with_context(|| format!("while loading {}", op.token()))?
        .with_context(|| format!("no such token {}", op.token()))?;
    match op {
        Operation::Init(_) => init(svc),
        Operation::Deinit(_) => deinit(svc),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use metalos_macros::containertest;
    use std::path::Path;

    pub(crate) fn wait_for_systemd() -> Result<()> {
        let mut proc = std::process::Command::new("systemctl")
            .arg("is-system-running")
            .arg("--wait")
            .spawn()?;
        proc.wait()?;
        Ok(())
    }

    #[containertest]
    fn test_init() -> Result<()> {
        wait_for_systemd()?;
        let svc = ServiceInstance::new(
            "metalos.demo".into(),
            "00000000000040008000000000000001".parse().unwrap(),
        );
        let run_uuid = svc.run_uuid();
        init(svc)?;
        assert!(
            Path::new(&format!(
                "/run/fs/control/run/service_roots/metalos.demo-{}-{}",
                "00000000000040008000000000000001",
                run_uuid.to_simple()
            ))
            .exists(),
        );
        Ok(())
    }
}
