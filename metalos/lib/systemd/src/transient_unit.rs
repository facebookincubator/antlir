/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::os::unix::io::RawFd;

use derive_builder::Builder;
use futures::future;
use futures::stream::StreamExt;
use maplit::hashmap;
use slog::debug;
use thiserror::Error;
use zbus::zvariant::Value;

use crate::property_stream::property_stream;
use crate::property_stream::PropertyStream;
use crate::systemd_manager::ActiveState;
use crate::systemd_manager::ServiceResult;
use crate::systemd_manager::StartMode;
use crate::systemd_manager::UnitName;
use crate::Result;
use crate::Systemd;

#[derive(Error, Debug)]
pub enum Error {
    #[error("failed creating the transient unit")]
    Create(zbus::Error),
    #[error("error waiting for transient unit")]
    Wait(zbus::Error),
    #[error("unexpected or invalid state transitions")]
    Transition(&'static str),
    #[error("transient unit failed")]
    Failed(ServiceResult),
    #[error("error Unref-ing transient unit")]
    Unref(zbus::Error),
}

#[derive(Debug, Builder)]
#[builder(setter(into, strip_option))]
pub struct Opts {
    // Full unit name, including the .service suffix
    unit_name: UnitName,
    // Unit description, defaults to unit_name
    #[builder(default = "self.default_description()?")]
    description: String,
    // Program to execute
    program: String,
    // Arguments to pass to the program (argv[0] will be prepended automatically
    // with the same value as [Opts::program])
    #[builder(default)]
    arguments: Vec<String>,
    #[builder(default)]
    stdin: Option<RawFd>,
    #[builder(default)]
    stdout: Option<RawFd>,
    #[builder(default)]
    stderr: Option<RawFd>,
}

impl OptsBuilder {
    fn default_description(&self) -> std::result::Result<String, String> {
        match &self.unit_name {
            Some(name) => Ok(name.to_string()),
            None => Err("unit_name must be set".to_string()),
        }
    }
}

impl Opts {
    pub fn builder() -> OptsBuilder {
        OptsBuilder::default()
    }
}

impl Systemd {
    /// Transient units are created with
    /// [ManagerProxy::start_transient_unit](crate::systemd_manager::ManagerProxy::start_transient_unit)
    /// and are automatically unloaded by systemd when they are finished and the
    /// dbus connection which created them is closed, or
    /// [UnitProxy::unref](crate::systemd_manager::UnitProxy::unref) is called.
    /// This abstraction will unload the unit when it is finished.  This method
    /// will create a transient unit and wait for it to finish, where it will
    /// return the result of the service unit. If the service finished, but the
    /// result was not
    /// [Success](crate::systemd_manager::ServiceResult::Success), this method
    /// will return [Error::Failed] with the final service result.
    pub async fn run_transient_unit(&self, opts: Opts) -> Result<ServiceResult> {
        let unit_name = opts.unit_name;
        let mut argv = vec![opts.program.clone()];
        argv.extend(opts.arguments);
        let mut properties = hashmap! {
            "Description" => Value::new(opts.description),
            "ExecStart" => Value::new(vec![(opts.program, argv, false)]),
            // this keeps the unit alive until the dbus connection closes
            "AddRef" => Value::new(true),
        };
        if let Some(stdin) = opts.stdin {
            properties.insert(
                "StandardInputFileDescriptor",
                Value::Fd(zbus::zvariant::Fd::from(stdin)),
            );
        }
        if let Some(stdout) = opts.stdout {
            properties.insert(
                "StandardOutputFileDescriptor",
                Value::Fd(zbus::zvariant::Fd::from(stdout)),
            );
        }
        if let Some(stderr) = opts.stderr {
            properties.insert(
                "StandardErrorFileDescriptor",
                Value::Fd(zbus::zvariant::Fd::from(stderr)),
            );
        }

        self.subscribe().await.map_err(Error::Wait)?;
        let job_removed_stream = self.receive_job_removed().await.map_err(Error::Wait)?;
        let job_path = self
            .start_transient_unit(
                &unit_name,
                &StartMode::Replace,
                &properties.into_iter().collect::<Vec<_>>(),
                &[],
            )
            .await
            .map_err(Error::Create)?;

        // wait for the job to finish before checking the service state
        job_removed_stream
            .filter(|j| future::ready(j.args().map(|args| args.job == job_path).unwrap_or(false)))
            .next()
            .await
            .ok_or(Error::Transition("never got JobRemoved"))?;

        // Now we can start a stream for the service result. Note that this has
        // to be a stream on the property change, since the job is removed
        // before the service is necessarily finished.
        let unit = self.get_unit(&unit_name).await.map_err(Error::Wait)?;

        // A service's Result cannot be adequately judged until its ActiveState
        // goes to 'inactive' or 'failed', since systemd starts out a service in
        // Result=Sucess, because nothing is ever easy
        let active_state_stream: PropertyStream<ActiveState> =
            property_stream!(self.log, unit, active_state, "ActiveState").await?;

        active_state_stream
            .filter(|s| future::ready(s == &ActiveState::Inactive || s == &ActiveState::Failed))
            .next()
            .await
            .ok_or(Error::Transition("never got completed ActiveState"))?;

        let service = self
            .get_service_unit(&unit_name)
            .await
            .map_err(Error::Wait)?;

        let mut result_stream: PropertyStream<ServiceResult> =
            property_stream!(self.log, service, result, "Result").await?;

        let result = result_stream
            .next()
            .await
            .ok_or(Error::Transition("never got service Result"))?;
        debug!(self.log, "{} Result -> {:?}", &unit_name, result);
        let ret = match result {
            ServiceResult::Success => Ok(ServiceResult::Success),
            _ => Err(Error::Failed(result).into()),
        };
        unit.unref().await.map_err(Error::Unref)?;
        ret
    }
}

#[cfg(test)]
mod tests {
    use super::Opts;
    use crate::MachineExt;
    use crate::Machined;
    use crate::ServiceResult;
    use crate::Systemd;
    use crate::WaitableSystemState;
    use anyhow::Result;
    use os_pipe::pipe;
    use std::io::Read;
    use std::os::unix::io::AsRawFd;
    use std::time::Duration;
    use tokio::time::sleep;
    use tokio::time::timeout;

    #[containertest]
    async fn test_transient_run() -> Result<()> {
        let log = slog::Logger::root(slog_glog_fmt::default_drain(), slog::o!());
        let sd = Systemd::connect(log.clone()).await?;
        sd.wait(WaitableSystemState::Operational).await?;
        let result = sd
            .run_transient_unit(
                Opts::builder()
                    .unit_name("example.service")
                    .program("/bin/echo")
                    .arguments(vec!["hello".into(), "world".into()])
                    .build()
                    .unwrap(),
            )
            .await?;
        assert_eq!(result, ServiceResult::Success);
        Ok(())
    }

    #[containertest]
    async fn test_transient_run_fail() -> Result<()> {
        let log = slog::Logger::root(slog_glog_fmt::default_drain(), slog::o!());
        let sd = Systemd::connect(log.clone()).await?;
        sd.wait(WaitableSystemState::Operational).await?;
        let err = sd
            .run_transient_unit(
                Opts::builder()
                    .unit_name("example.service")
                    .program("/bin/sh")
                    .arguments(vec!["-c".into(), "sleep 1; exit 1".into()])
                    .build()
                    .unwrap(),
            )
            .await
            .unwrap_err();
        assert!(
            err.to_string().contains("Failed(ExitCode)"),
            "{} did not contain Failed(ExitCode)",
            err.to_string()
        );
        Ok(())
    }

    #[containertest]
    async fn test_transient_run_pipes() -> Result<()> {
        let log = slog::Logger::root(slog_glog_fmt::default_drain(), slog::o!());
        let sd = Systemd::connect(log.clone()).await?;
        sd.wait(WaitableSystemState::Operational).await?;
        let (mut r, w) = pipe()?;
        sd.run_transient_unit(
            Opts::builder()
                .unit_name("example.service")
                .program("/bin/echo")
                .arguments(vec!["hello".into(), "world".into()])
                .stdout(w.as_raw_fd())
                .build()
                .unwrap(),
        )
        .await?;
        drop(w);
        let mut output = String::new();
        r.read_to_string(&mut output)?;
        assert_eq!(output, "hello world\n");
        Ok(())
    }

    #[containertest]
    async fn test_transient_run_in_container() -> Result<()> {
        let log = slog::Logger::root(slog_glog_fmt::default_drain(), slog::o!());
        let sd = Systemd::connect(log.clone()).await?;
        sd.wait(WaitableSystemState::Operational).await?;

        let mut container = std::process::Command::new("systemd-nspawn")
            .arg("--directory=/")
            .arg("--boot")
            .arg("--ephemeral")
            .arg("--machine=containermachine")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()?;

        let md = Machined::connect(log.clone()).await?;

        let machine = timeout(Duration::from_secs(1), async {
            loop {
                match md.get_machine(&"containermachine".into()).await {
                    Ok(machine) => return machine,
                    Err(e) => {
                        eprintln!("machine still not up: {:?}", e);
                        sleep(Duration::from_millis(50)).await;
                    }
                }
            }
        })
        .await?;
        let machine_sd = machine.systemd(log).await?;
        machine_sd.wait(WaitableSystemState::Starting).await?;

        let (mut r, w) = pipe()?;
        machine_sd
            .run_transient_unit(
                Opts::builder()
                    .unit_name("example.service")
                    .program("/bin/echo")
                    .arguments(vec!["hello".into(), "world".into()])
                    .stdout(w.as_raw_fd())
                    .build()
                    .unwrap(),
            )
            .await?;
        drop(w);

        let mut output = String::new();
        r.read_to_string(&mut output)?;
        assert_eq!(output, "hello world\n");
        let _ = container.kill();
        Ok(())
    }
}
