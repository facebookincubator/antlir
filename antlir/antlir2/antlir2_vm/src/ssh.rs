/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashMap;
use std::io::Write;
use std::net::Ipv6Addr;
use std::process::Command;
use std::str::FromStr;
use std::sync::Arc;

use once_cell::sync::OnceCell;
use tempfile::NamedTempFile;
use thiserror::Error;

#[derive(Error, Debug)]
pub(crate) enum GuestSSHError {
    #[error("Error writing private key to file: {0}")]
    PrivKey(std::io::Error),
}

type Result<T> = std::result::Result<T, GuestSSHError>;

/// Struct to represent command to be executed inside guest VM over SSH.
/// Can be reused.
pub(crate) struct GuestSSHCommand {
    /// ssh_config options for connection
    options: HashMap<String, String>,
    /// ssh client private key file
    privkey: Arc<NamedTempFile>,
}

static PRIVKEY: OnceCell<Arc<NamedTempFile>> = OnceCell::new();

impl GuestSSHCommand {
    /// Creates a new `GuestSSHCommand` with default options
    pub(crate) fn new() -> Result<GuestSSHCommand> {
        let privkey = PRIVKEY
            .get_or_try_init(|| {
                let mut privkey = NamedTempFile::new().map_err(GuestSSHError::PrivKey)?;
                privkey
                    .write_all(include_bytes!("./ssh/privkey"))
                    .map_err(GuestSSHError::PrivKey)?;
                Ok(Arc::new(privkey))
            })?
            .clone();

        Ok(GuestSSHCommand {
            options: [
                ("UserKnownHostsFile", "/dev/null"),
                ("StrictHostKeyChecking", "no"),
                ("ConnectTimeout", "10"),
                ("ConnectionAttempts", "3"),
                ("StreamLocalBindUnlink", "yes"),
            ]
            .iter()
            .map(|(x, y)| (x.to_string(), y.to_string()))
            .collect(),
            privkey,
        })
    }

    /// Set or override SSH connection options. See `man ssh_config` for details.
    #[allow(unused)]
    pub(crate) fn option(&mut self, name: String, value: String) -> &mut Self {
        self.options.insert(name, value);
        self
    }

    /// Return a `Command` that sshes into the guest VM.
    pub fn ssh_cmd(&self) -> Command {
        let mut command = Command::new("ssh");
        self.options.iter().for_each(|(name, value)| {
            command.arg("-o").arg(format!("{}={}", name, value));
        });
        command.arg("-i").arg(self.privkey.path());
        command.arg(format!("root@{}%vm0", self.guest_ipv6_addr_ll()));
        command
    }

    /// Link-local IP address of the first NIC of the guest VM. We always use this
    /// to communicate to VM. We use link-local address so that VM OS doesn't have
    /// to open up firewall for some global address specific for VM testing.
    fn guest_ipv6_addr_ll(&self) -> Ipv6Addr {
        Ipv6Addr::from_str("fe80::200:ff:fe00:1").expect("Invalid IPv6 address")
    }
}

#[cfg(test)]
mod test {
    use super::*;

    /// Flatten `Command` args to make asserts easier.
    fn get_args(cmd: &Command) -> String {
        let args: Option<Vec<&str>> = cmd.get_args().map(|x| x.to_str()).collect();
        args.expect("Invalid string in command args").join(" ")
    }

    /// Expose fields for testing purpose.
    impl GuestSSHCommand {
        fn get_options(&self) -> &HashMap<String, String> {
            &self.options
        }
        fn get_key(&self) -> &str {
            self.privkey
                .path()
                .to_str()
                .expect("Invalid private key path")
        }
    }

    /// Bypass normal `new` due to checks that may not hold for unit tests
    fn new() -> GuestSSHCommand {
        let privkey = Arc::new(NamedTempFile::new().expect("Failed to create temp file"));
        GuestSSHCommand {
            options: [
                ("UserKnownHostsFile", "/dev/null"),
                ("StrictHostKeyChecking", "no"),
                ("ConnectTimeout", "1"),
                ("ConnectionAttempts", "3"),
                ("StreamLocalBindUnlink", "yes"),
            ]
            .iter()
            .map(|(x, y)| (x.to_string(), y.to_string()))
            .collect(),
            privkey,
        }
    }

    #[test]
    fn test_ssh_cmd() {
        let mut ssh = new();
        // default options
        ssh.get_options().iter().for_each(|(name, value)| {
            assert!(get_args(&ssh.ssh_cmd()).contains(&format!("-o {}={}", name, value)));
        });
        assert!(get_args(&ssh.ssh_cmd()).contains(&format!("-i {}", ssh.get_key())));
        assert!(get_args(&ssh.ssh_cmd()).contains("root@fe80::200:ff:fe00:1"));

        // option override
        ssh.option(
            "UserKnownHostsFile".to_string(),
            "/dev/whatever".to_string(),
        );
        ssh.get_options().iter().for_each(|(name, value)| {
            assert!(get_args(&ssh.ssh_cmd()).contains(&format!("-o {}={}", name, value)));
        });
        assert!(get_args(&ssh.ssh_cmd()).contains("-o UserKnownHostsFile=/dev/whatever"));
        assert!(!get_args(&ssh.ssh_cmd()).contains("-o UserKnownHostsFile=/dev/null"));

        // new option
        ssh.option("Whatever".to_string(), "hello".to_string());
        ssh.get_options().iter().for_each(|(name, value)| {
            assert!(get_args(&ssh.ssh_cmd()).contains(&format!("-o {}={}", name, value)));
        });
        assert!(get_args(&ssh.ssh_cmd()).contains("-o Whatever=hello"));
    }
}
