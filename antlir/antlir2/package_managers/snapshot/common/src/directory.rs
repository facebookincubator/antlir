/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeMap;
use std::collections::VecDeque;
use std::fs::File;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use cap_std::fs::Dir;

use crate::Checksums;

/// Walk a directory tree without following symlinks, returning
/// `(full_path, relpath_key)` for every regular file.
pub fn directory_files(tree: &Path) -> Result<Vec<(PathBuf, String)>> {
    let dir = Dir::open_ambient_dir(tree, cap_std::ambient_authority())
        .with_context(|| format!("failed to open directory {}", tree.display()))?;

    // BFS walk without following symlinks for containment. cap-std also
    // ensures the walk never escapes the tree root through a symlink.
    let mut entries = Vec::new();
    let mut queue: VecDeque<PathBuf> = VecDeque::new();
    queue.push_back(PathBuf::new());

    while let Some(rel_dir) = queue.pop_front() {
        // Failing to open any directory fails the whole operation:
        // silently dropping a subtree would produce incomplete
        // checksums (and, in the upload path, an incomplete tree).
        let subdir = if rel_dir.as_os_str().is_empty() {
            dir.open_dir(".").context("failed to open root dir")?
        } else {
            dir.open_dir(&rel_dir)
                .with_context(|| format!("failed to open directory {}", rel_dir.display()))?
        };

        for entry_result in subdir.entries()? {
            let entry = entry_result?;
            let file_name = entry.file_name();
            let file_name = file_name
                .to_str()
                .ok_or_else(|| anyhow::anyhow!("non-utf8 entry {:?}", file_name))?;
            let entry_path = if rel_dir.as_os_str().is_empty() {
                PathBuf::from(file_name)
            } else {
                rel_dir.join(file_name)
            };
            let metadata = entry.metadata()?;
            if metadata.is_symlink() {
                continue;
            }
            if metadata.is_dir() {
                queue.push_back(entry_path);
            } else if metadata.is_file() {
                let key = entry_path
                    .to_str()
                    .ok_or_else(|| anyhow::anyhow!("non-utf8 path {:?}", entry_path))?
                    .to_owned();
                entries.push((tree.join(&entry_path), key));
            }
        }
    }

    Ok(entries)
}

/// Compute checksums for every regular file below a directory.
pub fn directory_checksums(tree: &Path) -> Result<BTreeMap<String, Checksums>> {
    directory_files(tree)?
        .into_iter()
        .map(|(path, key)| {
            let file =
                File::open(&path).with_context(|| format!("failed to open {}", path.display()))?;
            let checksums = Checksums::from_reader(file)
                .with_context(|| format!("failed to checksum {}", path.display()))?;
            Ok((key, checksums))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn directory_checksums_are_recursive() {
        let tree = std::env::temp_dir().join(format!(
            "snapshot-common-directory-test-{}",
            std::process::id()
        ));
        let nested = tree.join("nested/deeper");
        fs::create_dir_all(&nested).expect("create test tree");
        fs::write(tree.join("top.txt"), b"top").expect("write top-level file");
        fs::write(nested.join("leaf.txt"), b"Hello world\n").expect("write nested file");

        let checksums = directory_checksums(&tree).expect("checksum directory");
        fs::remove_dir_all(&tree).ok();

        assert_eq!(checksums.len(), 2);
        assert_eq!(
            checksums["nested/deeper/leaf.txt"].sha256_hex().as_deref(),
            Some("1894a19c85ba153acbf743ac4e43fc004c891604b26f8c69e1e83ea2afc7c48f")
        );
        assert!(checksums.contains_key("top.txt"));
    }

    #[test]
    fn directory_checksums_fail_on_missing_directory() {
        let missing = std::env::temp_dir().join(format!(
            "snapshot-common-directory-missing-{}",
            std::process::id()
        ));
        assert!(directory_checksums(&missing).is_err());
        assert!(directory_files(&missing).is_err());
    }
}
