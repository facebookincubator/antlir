/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#[cfg(not(target_os = "linux"))]
compile_error!("only supported on linux");

use std::ffi::OsStr;
use std::io::ErrorKind;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::Command;

use antlir2_users::passwd::EtcPasswd;
use isolate_cfg::InvocationType;
use isolate_cfg::IsolationContext;
use isolate_unshare_preexec::isolate_unshare_preexec;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("unsupported setting: {0}")]
    UnsupportedSetting(&'static str),
    #[error(transparent)]
    IO(#[from] std::io::Error),
    #[error("parsing user database: {0}")]
    UserDb(#[from] antlir2_users::Error),
    #[error("user '{0}' not found in user database")]
    MissingUser(String),
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
            devtmpfs,
            hostname,
            readonly,
        } = &self.0;

        // TODO: remove these settings entirely when we get rid of
        // systemd-nspawn / move the things that require this (like image_test)
        // to *only* use systemd-nspawn
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
            let dst = if let Ok(target) =
                std::fs::read_link(layer.join(dst.strip_prefix("/").unwrap_or(dst)))
            {
                dst.parent().unwrap_or(dst).join(target)
            } else {
                dst.clone().into_owned()
            };
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
        for devtmpfs in devtmpfs {
            for dev in ["null", "random", "urandom"] {
                file_binds.push(isolate_unshare_preexec::Bind {
                    src: Path::new("/dev").join(dev),
                    dst: Path::new(isolate_unshare_preexec::NEWROOT)
                        .join(devtmpfs.strip_prefix("/").unwrap_or(devtmpfs))
                        .join(dev),
                    ro: false,
                });
            }
        }

        let (uid, gid) = if user == "root" {
            (0, 0)
        } else {
            match std::fs::read_to_string(layer.join("etc/passwd")) {
                Ok(contents) => {
                    let user_db = EtcPasswd::parse(&contents).map_err(Error::UserDb)?;
                    user_db
                        .get_user_by_name(user)
                        .ok_or_else(|| Error::MissingUser(user.clone().into()))
                        .map(|u| (u.uid.into(), u.gid.into()))
                }
                Err(e) => match e.kind() {
                    ErrorKind::NotFound => Err(Error::MissingUser(user.clone().into())),
                    _ => Err(Error::IO(e)),
                },
            }?
        };

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
            devtmpfs: devtmpfs
                .iter()
                .map(|t| {
                    Path::new(isolate_unshare_preexec::NEWROOT)
                        .join(t.strip_prefix("/").unwrap_or(&t))
                        .to_owned()
                })
                .collect(),
            working_dir: working_directory
                .as_ref()
                .map(|wd| wd.clone().into())
                .or_else(|| std::env::current_dir().ok())
                .expect("no working dir set"),
            hostname: hostname.clone().map(|h| h.clone().into()),
            uid,
            gid,
            ephemeral: *ephemeral,
        };
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
