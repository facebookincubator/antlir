/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */
use std::collections::BTreeMap;
use std::path::Path;
use std::time::SystemTime;

use anyhow::Context;
use anyhow::Result;

pub type Username = String;

#[derive(Debug)]
pub struct ShadowRecord {
    user: Username,
    hash: String,
    timestamp: u64,
    aging_info: String,
}

impl ShadowRecord {
    pub fn new(user: Username, hash: String) -> Result<Self> {
        Ok(ShadowRecord {
            user,
            hash,
            timestamp: SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .context("Failed to get time since EPOCH")?
                .as_secs()
                / 86400, // 1 day
            aging_info: "0:99999:7:::".to_string(),
        })
    }

    fn to_shadow_line(&self) -> String {
        format!(
            "{}:{}:{}:{}\n",
            self.user, self.hash, self.timestamp, self.aging_info
        )
    }
}

impl TryFrom<&str> for ShadowRecord {
    type Error = anyhow::Error;

    fn try_from(shadow_content: &str) -> Result<Self, Self::Error> {
        let (user, remainder) = shadow_content
            .trim()
            .split_once(":")
            .context("Failed to split user and remainder")?;
        let (hash, remainder) = remainder
            .split_once(":")
            .context("Failed to split hash and remainder")?;
        let (timestamp, aging_info) = remainder
            .split_once(":")
            .context("Failed to split timestamp and aging_info")?;

        Ok(ShadowRecord {
            user: user.to_string(),
            hash: hash.to_string(),
            timestamp: timestamp.parse().context("timestamp was not a valid int")?,
            aging_info: aging_info.to_string(),
        })
    }
}

#[derive(Debug)]
pub struct ShadowFile {
    pub entries: BTreeMap<Username, ShadowRecord>,
}

impl ShadowFile {
    pub fn from_file(shadow_file: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(shadow_file)
            .context(format!("Can't read {:?}", shadow_file))?;

        content.as_str().try_into()
    }

    pub fn write_to_file(&self, shadow_file: &Path) -> Result<String> {
        let content: String = self.into();
        std::fs::write(shadow_file, &content).context("failed to write contents to file")?;
        Ok(content)
    }

    pub fn update_record(&mut self, shadow_record: ShadowRecord) {
        self.entries
            .insert(shadow_record.user.clone(), shadow_record);
    }
}

impl From<&ShadowFile> for String {
    fn from(shadow: &ShadowFile) -> String {
        let mut out = String::new();
        for (_, entry) in shadow.entries.iter() {
            out.push_str(&entry.to_shadow_line());
        }
        out
    }
}

impl TryFrom<&str> for ShadowFile {
    type Error = anyhow::Error;

    fn try_from(shadow_content: &str) -> Result<Self, Self::Error> {
        let mut entries = BTreeMap::new();

        for line in shadow_content.lines() {
            let record: ShadowRecord = line
                .try_into()
                .context(format!("failed to parse shadow record: {:?}", line))?;
            entries.insert(record.user.clone(), record);
        }

        Ok(ShadowFile { entries })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shadow_record_new() -> Result<()> {
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .context("Failed to get time since EPOCH")?
            .as_secs()
            / 86400; // 1 day

        let record = ShadowRecord::new(
            "test_user".to_string(),
            "$6$part1.$part2.part3.part4.".to_string(),
        )
        .context("failed to make shadow row")?;

        assert_eq!(&record.user, "test_user");
        assert_eq!(&record.hash, "$6$part1.$part2.part3.part4.");

        // It's very unlikely but we could run right on a day boundary
        assert!(record.timestamp == now || record.timestamp == now + 1);

        let row = record.to_shadow_line();
        assert_eq!(
            row,
            format!(
                "test_user:$6$part1.$part2.part3.part4.:{}:0:99999:7:::\n",
                record.timestamp,
            )
        );

        Ok(())
    }

    fn test_mutate<SETUP, GET>(setup_fn: SETUP, get_fn: GET) -> Result<()>
    where
        // Takes in a shadow file content and gives back a built
        // shadow file
        SETUP: Fn(&str) -> Result<ShadowFile>,
        // Takes in a shadow file and gives back a shadow file content
        GET: Fn(&ShadowFile) -> Result<String>,
    {
        let content = r#"
adm:*:18397:0:99999:7:::
daemon:*:18397:0:99999:7:::
nobody:*:18397:0:99999:7:::
operator:*:18397:0:99999:7:::
root:$6$Y76FGZNuyp0WJ.K5$vwEc9SYniwXzDXyLJs66FD1A3DOLYsct1EgIBu45J5O71i4Tl9jnTQWVxZwx3MFqHO8s7Yszwgm7PBfqBPIvA1:18983:0:99999:7:::
test_user:$6$9Fbg5AzdDP6iLGf1$1U2RA4T7GMNHv9qccf4a9V.B/jXz.G1BhFg9NbELUPXVQdnNBT17SBK1SYPmCRNeCrUPhWuavnD9AQMUfz1ng1:18983:0:99999:7:::
shutdown:*:18397:0:99999:7:::   
sshd:!!:18927::::::
"#.trim_start();

        let mut file = setup_fn(content).context("Failed to setup test")?;

        assert_eq!(
            get_fn(&file).context("Failed to get new content from unchanged file")?,
            r#"
adm:*:18397:0:99999:7:::
daemon:*:18397:0:99999:7:::
nobody:*:18397:0:99999:7:::
operator:*:18397:0:99999:7:::
root:$6$Y76FGZNuyp0WJ.K5$vwEc9SYniwXzDXyLJs66FD1A3DOLYsct1EgIBu45J5O71i4Tl9jnTQWVxZwx3MFqHO8s7Yszwgm7PBfqBPIvA1:18983:0:99999:7:::
shutdown:*:18397:0:99999:7:::
sshd:!!:18927::::::
test_user:$6$9Fbg5AzdDP6iLGf1$1U2RA4T7GMNHv9qccf4a9V.B/jXz.G1BhFg9NbELUPXVQdnNBT17SBK1SYPmCRNeCrUPhWuavnD9AQMUfz1ng1:18983:0:99999:7:::
"#.trim_start(),
        );

        file.update_record(ShadowRecord {
            user: "new_user".to_string(),
            hash: "$6$part1.$part2.part3.part4.".to_string(),
            timestamp: 12345,
            aging_info: "0:99999:7:::".to_string(),
        });

        file.update_record(ShadowRecord {
            user: "test_user".to_string(),
            hash: "unit_test_hash".to_string(),
            timestamp: 45678,
            aging_info: "0:99999:7:::".to_string(),
        });

        assert_eq!(
            get_fn(&file).context("Failed to get new content from mutated file")?,
            r#"
adm:*:18397:0:99999:7:::
daemon:*:18397:0:99999:7:::
new_user:$6$part1.$part2.part3.part4.:12345:0:99999:7:::
nobody:*:18397:0:99999:7:::
operator:*:18397:0:99999:7:::
root:$6$Y76FGZNuyp0WJ.K5$vwEc9SYniwXzDXyLJs66FD1A3DOLYsct1EgIBu45J5O71i4Tl9jnTQWVxZwx3MFqHO8s7Yszwgm7PBfqBPIvA1:18983:0:99999:7:::
shutdown:*:18397:0:99999:7:::
sshd:!!:18927::::::
test_user:unit_test_hash:45678:0:99999:7:::
"#.trim_start(),
        );

        Ok(())
    }

    #[test]
    fn test_shadow_record_parse() -> Result<()> {
        let line = "test_user:$6$part1.$part2.part3.part4.:12345:0:99999:7:::\n";

        let record: ShadowRecord = line.try_into().context("failed to parse testing str")?;

        assert_eq!(&record.user, "test_user");
        assert_eq!(&record.hash, "$6$part1.$part2.part3.part4.");
        assert_eq!(record.timestamp, 12345);
        assert_eq!(&record.aging_info, "0:99999:7:::");

        let row = record.to_shadow_line();
        assert_eq!(&row, line);

        Ok(())
    }

    #[test]
    fn test_shadow_file_string() -> Result<()> {
        test_mutate(
            |content| content.try_into(),
            |shadow_file| Ok(shadow_file.into()),
        )
    }

    #[test]
    fn test_shadow_read_write_file() -> Result<()> {
        let ts = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .context("Failed to get timestamp")?;
        let tmpdir = std::env::temp_dir().join(format!("test_shadow_read_write_file_{:?}", ts));
        std::fs::create_dir(&tmpdir).context("failed to create tmpdir")?;

        let test_path = tmpdir.join("shadow");
        let test_output_path = tmpdir.join("shadow_new");

        test_mutate(
            |content| {
                std::fs::write(&test_path, &content).context("failed to write test file")?;
                ShadowFile::from_file(&test_path).context("Failed to read test file")
            },
            |shadow_file| {
                shadow_file
                    .write_to_file(&test_output_path)
                    .context("failed to write shadow file")?;
                std::fs::read_to_string(&test_output_path)
                    .context("failed to reread the shadow file")
            },
        )
    }
}
