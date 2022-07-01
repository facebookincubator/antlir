/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use crate::UnitName;

/// Escape a string for use in a systemd unit name.
/// For details on the escaping algorithm, see systemd.unit(5)
pub fn escape(s: impl AsRef<str>) -> UnitName {
    let mut escaped = String::with_capacity(s.as_ref().len() * 2);
    for byte in s.as_ref().as_bytes() {
        let ch = *byte as char;
        match ch {
            '/' => escaped.push('-'),
            ':' | '_' | '.' => escaped.push(ch),
            _ => match ch.is_ascii_alphanumeric() {
                true => escaped.push(ch),
                false => escaped.push_str(&format!("\\x{:02x}", byte)),
            },
        };
    }
    escaped.into()
}

/// Escape an instance variable for use in a systemd template unit instance.
pub fn template_unit_name(
    template: impl AsRef<str>,
    instance: impl AsRef<str>,
    suffix: impl AsRef<str>,
) -> UnitName {
    format!(
        "{}@{}.{}",
        template.as_ref(),
        escape(instance),
        suffix.as_ref()
    )
    .into()
}

#[cfg(test)]
mod tests {
    use super::escape;
    use super::template_unit_name;
    use anyhow::bail;
    use anyhow::Context;
    use anyhow::Result;
    use std::process::Command;

    #[test]
    fn escape_smoketest() -> Result<()> {
        assert_eq!(escape("plainstring"), "plainstring");
        assert_eq!(
            escape("https://[2401:db00:2120:20f2:face:0:9:0]:3000/initrd"),
            "https:--\\x5b2401:db00:2120:20f2:face:0:9:0\\x5d:3000-initrd"
        );
        Ok(())
    }

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

    #[test]
    fn escape_compare() -> Result<()> {
        for s in &[
            "hello",
            "https://[2401:db00:2120:20f2:face:0:9:0]:3000/initrd",
            "💖",
            "a💖b",
            "with-dashes",
        ] {
            let sd = run_systemd_escape(&[s])?;
            let mine = escape(s);
            assert_eq!(mine, sd, "{}: expected {}, got {}", s, sd, mine);
        }
        Ok(())
    }

    #[test]
    fn test_template() -> Result<()> {
        assert_eq!(
            template_unit_name("metalos-fetch-image", "https://some/url", "service"),
            "metalos-fetch-image@https:--some-url.service",
        );
        Ok(())
    }
}
