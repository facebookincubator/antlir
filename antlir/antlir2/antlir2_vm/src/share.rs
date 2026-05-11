/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::env;
use std::ffi::OsString;
use std::fs;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use std::process::Child;
use std::process::Command;
use std::str::FromStr;

use thiserror::Error;
use tracing::Level;
use tracing_subscriber::filter::LevelFilter;

use crate::types::QemuDevice;
use crate::types::ShareOpts;
use crate::utils::log_command;

#[derive(Debug, Error)]
pub(crate) enum ShareError {
    #[error("Invalid mount tag: `{0}`")]
    InvalidMountTagError(String),
    #[error("Virtiofsd failed to start: `{0}`")]
    VirtiofsdError(std::io::Error),
    #[error("Failed to generate mount unit file for shares: `{0}`")]
    MountUnitGenerationError(std::io::Error),
    #[error("Failed to create unit files directory: `{0}`")]
    UnitFilesDirError(std::io::Error),
    #[error("No directory is being shared")]
    EmptyShareError,
}

type Result<T> = std::result::Result<T, ShareError>;

pub(crate) trait Share: QemuDevice {
    /// Create Share based on full set of ShareOpts
    fn new(opts: ShareOpts, id: usize, state_dir: PathBuf) -> Self;
    /// Run any necessary setup to enable the Share
    fn setup(&self) -> Result<()>;
    /// Mount `Options` string for the mount unit
    fn mount_options(&self) -> String;

    // Boilerplate getters
    fn get_mount_type(&self) -> &str;
    fn get_id(&self) -> usize;
    fn get_opts(&self) -> &ShareOpts;

    // The following methods should be same across different mount types

    /// Generate file name according to systemd.mount(5)
    fn mount_unit_name(&self) -> Result<String> {
        let output = Command::new("systemd-escape")
            .arg("--suffix=mount")
            .arg("--path")
            .arg(&self.get_opts().path)
            .output()
            .map_err(|_| {
                ShareError::InvalidMountTagError(self.get_opts().path.to_string_lossy().to_string())
            })?;
        Ok(std::str::from_utf8(&output.stdout)
            .map_err(|_| {
                ShareError::InvalidMountTagError(self.get_opts().path.to_string_lossy().to_string())
            })?
            .trim()
            .to_string())
    }

    /// Generate mount tag with id unless it's specified
    fn mount_tag(&self) -> String {
        match &self.get_opts().mount_tag {
            Some(tag) => tag.clone(),
            None => format!("fs{}", self.get_id()),
        }
    }

    /// Generate .mount unit content
    fn mount_unit_content(&self) -> String {
        format!(
            r#"[Unit]
Description=Mount {tag} at {mountpoint}
Requires=systemd-modules-load.service
After=systemd-modules-load.service
Before=local-fs.target

[Mount]
What={tag}
Where={mountpoint}
Type={mount_type}
Options={mount_options}"#,
            tag = self.mount_tag(),
            mountpoint = self.get_opts().path.to_str().expect("Invalid UTF-8"),
            mount_type = self.get_mount_type(),
            mount_options = self.mount_options(),
        )
    }
}

macro_rules! share_getters {
    () => {
        fn get_id(&self) -> usize {
            self.id
        }
        fn get_opts(&self) -> &ShareOpts {
            &self.opts
        }
        fn get_mount_type(&self) -> &str {
            self.mount_type
        }
    };
}

/// `ViriofsShare` setups sharing through virtiofs
#[derive(Debug, Default)]
pub(crate) struct VirtiofsShare {
    /// User specified options for the share
    opts: ShareOpts,
    /// Index of the share, used to generate unique mount tag, chardev name
    /// and socket file.
    id: usize,
    /// State directory
    state_dir: PathBuf,
    /// Mount type
    mount_type: &'static str,
}

impl Share for VirtiofsShare {
    share_getters!();

    fn new(opts: ShareOpts, id: usize, state_dir: PathBuf) -> Self {
        Self {
            opts,
            id,
            state_dir,
            mount_type: "virtiofs",
        }
    }

    fn setup(&self) -> Result<()> {
        self.start_virtiofsd()?;
        Ok(())
    }

    fn mount_options(&self) -> String {
        if self.get_opts().read_only {
            "ro"
        } else {
            "rw"
        }
        .to_owned()
    }
}

impl QemuDevice for VirtiofsShare {
    fn qemu_args(&self) -> Vec<OsString> {
        [
            "-chardev",
            &format!(
                "socket,id={},path={}",
                self.chardev_node(),
                self.socket_path()
                    .to_str()
                    .expect("socket file should be valid string"),
            ),
            "-device",
            &format!(
                "vhost-user-fs-pci,queue-size=1024,chardev={},tag={}",
                self.chardev_node(),
                self.mount_tag(),
            ),
        ]
        .iter()
        .map(|x| x.into())
        .collect()
    }
}

impl VirtiofsShare {
    fn chardev_node(&self) -> String {
        format!("fs_chardev{}", self.id)
    }

    fn socket_path(&self) -> PathBuf {
        self.state_dir.join(self.mount_tag())
    }

    /// Rust virtiofsd seems to print out every request it gets on debug level,
    /// rendering terminal unusable. While users can craft the `RUST_LOG` env to
    /// suppress it manually, let's just set a more sensible level for it by
    /// default. We still honor level explicitly set for virtiofsd in case one
    /// really wants it.
    fn virtiofsd_log_level(&self) -> Option<&'static str> {
        let reduced = "warn";
        match env::var("RUST_LOG") {
            Ok(v) => {
                if v.split(',').any(|s| s.starts_with("virtiofsd=")) {
                    // Honor level set explicitly for virtiofsd
                    None
                } else if LevelFilter::current()
                    > Level::from_str(reduced)
                        .unwrap_or_else(|_| panic!("Level {} must be valid", reduced))
                {
                    Some(reduced)
                } else {
                    None
                }
            }
            Err(_) => Some(reduced),
        }
    }

    /// Virtiofs requires one virtiofsd for each shared path. This command assumes
    /// it's running as root inside container.
    pub(crate) fn start_virtiofsd(&self) -> Result<Child> {
        let mut command = Command::new("/usr/libexec/virtiofsd");
        if let Some(lv) = self.virtiofsd_log_level() {
            // Override logging level for virtiofsd
            command.env("RUST_LOG", lv);
        }
        log_command(
            command
                .arg("--socket-path")
                .arg(self.socket_path())
                .arg("--shared-dir")
                .arg(&self.opts.path)
                .arg("--cache")
                .arg("always"),
        )
        .spawn()
        .map_err(ShareError::VirtiofsdError)
    }
}

/// This sets up all the shared directories from the host that need to be accessible by the VM.
/// There used to be more than one type of `Share` and thus the template, but we only have
/// `VirtiofsShare` now. Leaving the template here in case we need to migrate to new hotness again
/// in the future.
#[derive(Debug)]
pub(crate) struct Shares<T: Share> {
    /// Directories to be shared into VM
    shares: Vec<T>,
    /// Memory size of the qemu VM. This should match -m parameter.
    /// This is used for memory-backend-file for virtiofsd shares.
    mem_mb: usize,
    /// Directory that holds unit files for other shares
    unit_files_dir: PathBuf,
    /// VirtiofsShare for setting up the `exports` mount. It contains all the generated unit files
    /// from `shares`. The guest OS contains a systemd generator that would mount `exports`, copy
    /// the unit files to the right place and setup dependency links, which instructs systemd to
    /// setup the mounts for `shares`.
    exports_share: VirtiofsShare,
    /// When true, extra_qemu_args provides custom NUMA memory backends.
    /// Skip the default single-node memory/NUMA setup.
    custom_numa: bool,
}

impl<T: Share> Shares<T> {
    pub(crate) fn new(
        shares: Vec<T>,
        mem_mb: usize,
        state_dir: PathBuf,
        custom_numa: bool,
    ) -> Result<Self> {
        if shares.is_empty() {
            return Err(ShareError::EmptyShareError);
        }
        // Create the unit files directory inside state_dir
        let unit_files_dir = state_dir.join("mount_units");
        fs::create_dir(&unit_files_dir).map_err(ShareError::UnitFilesDirError)?;
        let exports_share = VirtiofsShare::new(
            ShareOpts {
                path: unit_files_dir.clone(),
                read_only: true,
                mount_tag: Some("exports".to_string()),
            },
            usize::MAX, // Use a large id that won't conflict with user shares
            state_dir,
        );
        Ok(Self {
            shares,
            mem_mb,
            unit_files_dir,
            custom_numa,
            exports_share,
        })
    }

    /// Write all unit files in the unit files directory
    pub(crate) fn generate_unit_files(&self) -> Result<()> {
        self.shares.iter().try_for_each(|share| {
            let name = share.mount_unit_name()?;
            let content = share.mount_unit_content().into_bytes();
            let mut file = File::create(self.unit_files_dir.join(name))
                .map_err(ShareError::MountUnitGenerationError)?;
            file.write_all(&content)
                .map_err(ShareError::MountUnitGenerationError)?;
            Ok(())
        })
    }

    pub(crate) fn start_shares(&self) -> Result<()> {
        self.shares.iter().try_for_each(|share| share.setup())?;
        // Start virtiofsd for exports using the VirtiofsShare
        self.exports_share.start_virtiofsd()?;
        Ok(())
    }

    /// Memory/NUMA setup for virtiofsd. When custom_numa is set,
    /// extra_qemu_args provides all memory backends (with share=on) and
    /// NUMA topology, so we emit nothing here.
    fn memory_file_qemu_args(&self) -> Vec<OsString> {
        if self.custom_numa {
            // extra_qemu_args provides all memory-backend-memfd objects
            // (each with share=on for virtiofsd) and NUMA node assignments.
            vec![]
        } else {
            [
                "-object",
                &format!("memory-backend-memfd,id=mem,share=on,size={}M", self.mem_mb),
                "-numa",
                "node,memdev=mem",
            ]
            .iter()
            .map(|x| x.into())
            .collect()
        }
    }
}

impl<T: Share> QemuDevice for Shares<T> {
    fn qemu_args(&self) -> Vec<OsString> {
        let mut args: Vec<_> = self.shares.iter().flat_map(|x| x.qemu_args()).collect();
        args.extend(self.exports_share.qemu_args());
        args.extend(self.memory_file_qemu_args());
        args
    }
}

#[cfg(test)]
mod test {
    use std::ffi::OsStr;
    use std::fs;

    use tempfile::tempdir;
    use tracing_subscriber::EnvFilter;

    use super::*;
    use crate::utils::qemu_args_to_string;

    #[test]
    fn test_virtiofs_share() {
        // Read-only mount without mount_tag
        let opts = ShareOpts {
            path: PathBuf::from("/this/is/a/test"),
            read_only: true,
            mount_tag: None,
        };
        let share = VirtiofsShare::new(opts, 3, PathBuf::from("/tmp/test"));

        assert_eq!(&share.mount_tag(), "fs3");
        assert_eq!(share.get_mount_type(), "virtiofs");
        assert_eq!(&share.mount_options(), "ro");
        assert_eq!(&share.chardev_node(), "fs_chardev3");
        assert_eq!(share.socket_path(), PathBuf::from("/tmp/test/fs3"));
        assert_eq!(
            share.mount_unit_name().expect("Invalid mount unit name"),
            "this-is-a-test.mount".to_string(),
        );
        let mount_unit_content = r#"[Unit]
Description=Mount fs3 at /this/is/a/test
Requires=systemd-modules-load.service
After=systemd-modules-load.service
Before=local-fs.target

[Mount]
What=fs3
Where=/this/is/a/test
Type=virtiofs
Options=ro"#;
        assert_eq!(&share.mount_unit_content(), mount_unit_content);
        assert_eq!(
            share.qemu_args().join(OsStr::new(" ")),
            "-chardev socket,id=fs_chardev3,path=/tmp/test/fs3 \
            -device vhost-user-fs-pci,queue-size=1024,chardev=fs_chardev3,tag=fs3",
        );

        // RW mount with custom mount_tag
        let opts = ShareOpts {
            path: PathBuf::from("/this/is/a/test"),
            read_only: false,
            mount_tag: Some("whatever".to_string()),
        };
        let share = VirtiofsShare::new(opts, 3, PathBuf::from("/tmp/test"));

        assert_eq!(&share.mount_tag(), "whatever");
        assert_eq!(&share.chardev_node(), "fs_chardev3");
        assert_eq!(share.socket_path(), PathBuf::from("/tmp/test/whatever"));
        assert_eq!(
            share.mount_unit_name().expect("Invalid mount unit name"),
            "this-is-a-test.mount".to_string(),
        );
        let mount_unit_content = r#"[Unit]
Description=Mount whatever at /this/is/a/test
Requires=systemd-modules-load.service
After=systemd-modules-load.service
Before=local-fs.target

[Mount]
What=whatever
Where=/this/is/a/test
Type=virtiofs
Options=rw"#;
        assert_eq!(&share.mount_unit_content(), mount_unit_content);
        assert_eq!(
            share.qemu_args().join(OsStr::new(" ")),
            "-chardev socket,id=fs_chardev3,path=/tmp/test/whatever \
            -device vhost-user-fs-pci,queue-size=1024,chardev=fs_chardev3,tag=whatever",
        );
    }

    #[test]
    fn test_shares() {
        let state_dir = tempdir().expect("Failed to create state tempdir for testing");
        let opts = ShareOpts {
            path: PathBuf::from("/this/is/a/test"),
            read_only: true,
            mount_tag: None,
        };
        let share = VirtiofsShare::new(opts, 3, state_dir.path().to_path_buf());
        let shares = Shares::new(vec![share], 1024, state_dir.path().to_path_buf(), false)
            .expect("Failed to create Shares");

        shares
            .generate_unit_files()
            .expect("Failed to generate unit files");

        let unit_files_dir = state_dir.path().join("mount_units");
        assert_eq!(
            fs::read_dir(&unit_files_dir)
                .expect("Failed to read unit_files_dir")
                .next()
                .expect("Missing expected file")
                .expect("Invalid directory entry")
                .file_name()
                .to_str()
                .expect("Invalid file name"),
            "this-is-a-test.mount",
        );

        assert_eq!(
            shares.memory_file_qemu_args().join(OsStr::new(" ")),
            "-object memory-backend-memfd,id=mem,share=on,size=1024M -numa node,memdev=mem",
        );
        // Test that qemu_args includes virtiofs args for exports
        let qemu_args = qemu_args_to_string(&shares.qemu_args());
        assert!(qemu_args.contains("exports"));
        assert!(qemu_args.contains("vhost-user-fs-pci"));
        let memory_file_qemu_args = qemu_args_to_string(&shares.memory_file_qemu_args());
        assert!(qemu_args.contains(&memory_file_qemu_args));
        shares.shares.iter().for_each(|x| {
            let share_args = qemu_args_to_string(&x.qemu_args());
            assert!(qemu_args.contains(&share_args))
        });
    }

    #[test]
    fn test_virtiofsd_log_level() {
        let share = VirtiofsShare::default();

        // no RUST_LOG
        if env::var_os("RUST_LOG").is_some() {
            // TODO: Audit that the environment access only happens in single-threaded code.
            unsafe { env::remove_var("RUST_LOG") };
            let _ = tracing::subscriber::set_default(
                tracing_subscriber::fmt()
                    .with_env_filter(EnvFilter::from_default_env())
                    .finish(),
            );
            assert_eq!(share.virtiofsd_log_level(), Some("warn"));
        }

        // RUST_LOG set at a more verbose level than what we want from virtiofsd
        // TODO: Audit that the environment access only happens in single-threaded code.
        unsafe { env::set_var("RUST_LOG", "debug") };
        let _ = tracing::subscriber::set_default(
            tracing_subscriber::fmt()
                .with_env_filter(EnvFilter::from_default_env())
                .finish(),
        );
        assert_eq!(share.virtiofsd_log_level(), Some("warn"));

        // RUST_LOG set to a less verbose level
        // TODO: Audit that the environment access only happens in single-threaded code.
        unsafe { env::set_var("RUST_LOG", "error") };
        let _ = tracing::subscriber::set_default(
            tracing_subscriber::fmt()
                .with_env_filter(EnvFilter::from_default_env())
                .finish(),
        );
        assert_eq!(share.virtiofsd_log_level(), None);

        // Explicit virtiofsd level
        // TODO: Audit that the environment access only happens in single-threaded code.
        unsafe { env::set_var("RUST_LOG", "debug,virtiofsd=info") };
        let _ = tracing::subscriber::set_default(
            tracing_subscriber::fmt()
                .with_env_filter(EnvFilter::from_default_env())
                .finish(),
        );
        assert_eq!(share.virtiofsd_log_level(), None);
    }
}
