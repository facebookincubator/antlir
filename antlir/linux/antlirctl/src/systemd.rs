/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::process::Command;

use anyhow::{bail, Context, Result};

pub static PROVIDER_ROOT: &str = "/usr/lib/systemd/system";

fn run_systemd_escape(args: &[&str]) -> Result<String> {
    let output = Command::new("/usr/bin/systemd-escape")
        .args(args)
        .output()
        .with_context(|| format!("'systemd-escape {:?}' failed", args))?;
    if !output.status.success() {
        let stderr = std::str::from_utf8(&output.stderr)
            .context("systemd-escape failed and output invalid UTF-8 on stderr")?;
        bail!("systemd-escape failed: {}", stderr);
    }
    std::str::from_utf8(&output.stdout)
        .context("systemd-escape returned invalid UTF-8")
        .map(|s| s.trim().to_owned())
}

pub fn escape<S: AsRef<str>>(s: S) -> Result<String> {
    run_systemd_escape(&[s.as_ref()])
}

pub fn template_unit_name<T: AsRef<str>, I: AsRef<str>>(
    template: T,
    instance: I,
) -> Result<String> {
    run_systemd_escape(&["--template", template.as_ref(), instance.as_ref()])
}

#[cfg(test)]
mod tests {
    use super::{escape, template_unit_name};
    use anyhow::Result;

    #[test]
    fn test_escape() -> Result<()> {
        assert_eq!(escape("plainstring")?, "plainstring");
        assert_eq!(escape("dev/sda")?, "dev-sda");
        assert_eq!(
            escape("https://[2401:db00:2120:20f2:face:0:9:0]:3000/initrd")?,
            "https:--\\x5b2401:db00:2120:20f2:face:0:9:0\\x5d:3000-initrd"
        );
        Ok(())
    }

    #[test]
    fn test_template() -> Result<()> {
        assert_eq!(
            template_unit_name("antlir-fetch-image@.service", "https://some/url")?,
            "antlir-fetch-image@https:--some-url.service"
        );
        Ok(())
    }
}
