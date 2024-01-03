/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#[cfg(not(target_os = "linux"))]
compile_error!("only supported on linux");

use std::ffi::OsStr;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::Command;

use isolate_cfg::InvocationType;
use isolate_cfg::IsolationContext;
use isolate_unshare_preexec::isolate_unshare_preexec;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("unsupported setting: {0}")]
    UnsupportedSetting(&'static str),
    #[error(transparent)]
    IO(#[from] std::io::Error),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug)]
pub struct IsolatedContext<'a>(IsolationContext<'a>);

impl<'a> IsolatedContext<'a> {
    #[deny(unused_variables)]
    pub fn command<S: AsRef<OsStr>>(&self, program: S) -> Result<Command> {
        let IsolationContext {
            layer,
            working_directory,
            setenv,
            platform,
            inputs,
            outputs,
            invocation_type,
            register,
            user,
            ephemeral,
            tmpfs,
            hostname,
            readonly,
        } = &self.0;

        // TODO: remove these settings entirely when we get rid of
        // systemd-nspawn / move the things that require this (like image_test)
        // to *only* use systemd-nspawn
        if *ephemeral {
            return Err(Error::UnsupportedSetting("ephemeral"));
        }
        if user != "root" {
            return Err(Error::UnsupportedSetting("user"));
        }
        if *invocation_type != InvocationType::Pid2Pipe {
            return Err(Error::UnsupportedSetting("invocation_type"));
        }
        if *register {
            return Err(Error::UnsupportedSetting("register"));
        }

        let mut cmd = Command::new(&program);

        cmd.env_clear();
        // reasonable default PATH (same as systemd-nspawn uses)
        cmd.env(
            "PATH",
            "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin",
        );
        cmd.env("container", "antlir2");
        cmd.env("USER", &**user);
        if let Some(term) = std::env::var_os("TERM") {
            cmd.env("TERM", term);
        }
        cmd.envs(setenv);

        let mut dir_binds = Vec::new();
        let mut file_binds = Vec::new();
        for (dst, src, ro) in inputs
            .iter()
            .chain(platform.iter())
            .map(|(dst, src)| (dst, src, true))
            .chain(outputs.iter().map(|(dst, src)| (dst, src, false)))
        {
            let ft = src.metadata()?.file_type();
            let dst = dst.canonicalize().unwrap_or_else(|_| dst.clone().into());
            let dst = Path::new(isolate_unshare_preexec::NEWROOT)
                .join(dst.strip_prefix("/").unwrap_or(&dst));
            if ft.is_dir() {
                dir_binds.push(isolate_unshare_preexec::Bind {
                    src: src.clone().into(),
                    dst,
                    ro,
                });
            } else {
                file_binds.push(isolate_unshare_preexec::Bind {
                    src: src.clone().into(),
                    dst,
                    ro,
                });
            }
        }

        let args = isolate_unshare_preexec::Args {
            root: layer.clone().into(),
            root_ro: *readonly,
            dir_binds,
            file_binds,
            tmpfs: tmpfs
                .iter()
                .map(|t| {
                    Path::new(isolate_unshare_preexec::NEWROOT)
                        .join(t.strip_prefix("/").unwrap_or(t))
                        .to_owned()
                })
                .collect(),
            working_dir: working_directory
                .as_ref()
                .map(|wd| wd.clone().into())
                .or_else(|| std::env::current_dir().ok())
                .expect("no working dir set"),
            hostname: hostname.clone().map(|h| h.clone().into()),
        };
        // let args = Box::leak(args);
        unsafe {
            cmd.pre_exec(move || isolate_unshare_preexec(&args).map_err(std::io::Error::from));
        }
        Ok(cmd)
    }
}

#[deny(unused_variables)]
pub fn prepare(ctx: IsolationContext) -> IsolatedContext {
    IsolatedContext(ctx)
}
