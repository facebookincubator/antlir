/*
 * Copyright (c) Meta Platforms, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#![feature(exit_status_error)]

use anyhow::Context;
use anyhow::Result;
use structopt::StructOpt;

use service::ServiceInstance;
use state::Alias;
use state::State;
use state::Token;

mod volumes;
use volumes::ServiceVolumes;
#[cfg(facebook)]
mod facebook;

#[derive(StructOpt)]
enum Operation {
    /// Prepare a service's subvolumes for execution. Must be invoked
    /// immediately before the service starts, via
    /// metalos-native-service@.service
    Init(SvcOpts),
    /// Cleanup subvolumes after a service execution. Must be invoked some time
    /// after the native service stops, via metalos-native-service@.service
    Deinit(SvcOpts),
    #[cfg(facebook)]
    #[structopt(flatten)]
    Facebook(facebook::Opts),
}

impl Operation {
    fn alias(&self) -> Alias<ServiceInstance> {
        match self {
            Self::Init(o) => o.service.clone(),
            Self::Deinit(o) => o.service.clone(),
            #[cfg(facebook)]
            Self::Facebook(o) => o.alias(),
        }
    }
}

#[derive(Debug, StructOpt)]
struct SvcOpts {
    /// Token pointing to a serialized ServiceInstance. Generated automatically
    /// as part of the native service lifecycle transitions.
    #[structopt(long)]
    service: Alias<ServiceInstance>,
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
    let log = slog::Logger::root(slog_glog_fmt::default_drain(), slog::o!());
    let op = Operation::from_args();
    let svc = ServiceInstance::aliased(op.alias())
        .with_context(|| format!("while loading {}", op.alias()))?
        .with_context(|| format!("no such token {}", op.alias()))?;
    match op {
        Operation::Init(_) => init(svc),
        Operation::Deinit(_) => deinit(svc),
        #[cfg(facebook)]
        Operation::Facebook(o) => facebook::main(log, o, svc),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use metalos_macros::containertest;
    use std::path::Path;

    #[containertest]
    async fn test_init() -> Result<()> {
        let svc = ServiceInstance::new(
            "metalos.service.demo".into(),
            "00000000000040008000000000000001".parse().unwrap(),
        );
        let run_uuid = svc.run_uuid();
        init(svc)?;
        assert!(
            Path::new(&format!(
                "/run/fs/control/run/service-roots/metalos.service.demo-{}-{}",
                "00000000000040008000000000000001",
                run_uuid.to_simple()
            ))
            .exists(),
        );
        Ok(())
    }
}
