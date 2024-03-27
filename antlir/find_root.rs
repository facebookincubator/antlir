/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::Path;
use std::path::PathBuf;

use thiserror::Error;

#[derive(Error, Debug)]
pub enum FindRootError {
    #[error("{0} not found in any ancestors of {1}")]
    SigilNotFound(&'static str, PathBuf),
}

pub fn find_repo_root(path_in_repo: impl AsRef<Path>) -> Result<PathBuf, FindRootError> {
    if let Ok("1") = std::env::var("INSIDE_RE_WORKER").as_deref() {
        return Ok("/re_cwd".into());
    }
    // Technically there is a bug here where we will return the first hg
    // repo found even if there is a git repo inside that hg repo.
    //
    // We are keeping this the same because this impl is trying to match the
    // artifacts_dir.py which has this bug.
    match first_parent_containing_sigil(path_in_repo.as_ref(), ".hg", true) {
        Some(path) => Ok(path),
        None => match first_parent_containing_sigil(path_in_repo.as_ref(), ".git", true) {
            Some(path) => Ok(path),
            None => match first_parent_containing_sigil(path_in_repo.as_ref(), ".sl", true) {
                Some(path) => Ok(path),
                None => Err(FindRootError::SigilNotFound(
                    ".hg, .git or .sl",
                    path_in_repo.as_ref().to_owned(),
                )),
            },
        },
    }
}

fn first_parent_containing_sigil(path: &Path, sigil_name: &str, is_dir: bool) -> Option<PathBuf> {
    for dir in path.ancestors() {
        let target_path = dir.join(sigil_name);
        if !target_path.exists() {
            continue;
        }

        if (is_dir && target_path.is_dir()) || (!is_dir && target_path.is_file()) {
            return Some(dir.to_owned());
        } else {
            continue;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use std::fs::create_dir;
    use std::fs::create_dir_all;
    use std::fs::File;

    use anyhow::anyhow;
    use anyhow::Result;
    use tempfile::TempDir;

    use super::*;

    fn make_tmp_dir() -> Result<TempDir> {
        let tmp_dir = TempDir::with_prefix("find_root_tests.")?;
        let path = tmp_dir.path();

        if let Some(p) = first_parent_containing_sigil(path, ".hg", true) {
            return Err(anyhow!(
                "Our temporary directory ({}) was created under an hg repo: {}",
                path.display(),
                p.display(),
            ));
        }
        if let Some(p) = first_parent_containing_sigil(path, ".git", true) {
            return Err(anyhow!(
                "Our temporary directory ({}) was created under a git repo: {}",
                path.display(),
                p.display(),
            ));
        }
        if let Some(p) = first_parent_containing_sigil(path, ".buckconfig", false) {
            return Err(anyhow!(
                "Our temporary directory ({}) was created under a buck repo: {}",
                path.display(),
                p.display(),
            ));
        }
        Ok(tmp_dir)
    }

    fn test_git_hg_common(sigil: &str) {
        let dot_sigil = format!(".{}", sigil);
        let tmp_dir = make_tmp_dir().expect("Failed to create tmp dir for test");
        let path = tmp_dir.path();

        // Before we create our sigil we should find no repo root
        assert!(find_repo_root(path).is_err());

        let sigil_root = path.join(sigil);
        create_dir(sigil_root.clone()).expect("failed to create subdir");

        // Having a directory git should still fail both outside and within it
        assert!(find_repo_root(path).is_err());
        assert!(find_repo_root(&sigil_root).is_err());

        // Now creating our sigil directory the root should still fail but it should
        // pass from inside now
        create_dir(sigil_root.join(&dot_sigil)).expect("failed to create subdirs");

        assert!(find_repo_root(path).is_err());
        assert_eq!(
            find_repo_root(&sigil_root)
                .expect("We should be able to find a repo root for in sigil subdir"),
            sigil_root,
        );

        // Test a deeply nested dir
        let shallow_dir = sigil_root.join("i/am");
        let mid_dir = shallow_dir.join("a/subdir");
        let deep_dir = mid_dir.join("of/the/repo");

        create_dir_all(&deep_dir).expect("Failed to make deep directory");
        assert_eq!(
            find_repo_root(&deep_dir)
                .expect("We should be able to find a repo root for in sigil subdir"),
            sigil_root,
        );

        create_dir(shallow_dir.join(&dot_sigil)).expect("failed to create sigil in shallow_dir");
        assert_eq!(
            find_repo_root(&shallow_dir)
                .expect("We should be able to find a repo root for in sigil subdir"),
            shallow_dir,
        );
        assert_eq!(
            find_repo_root(&deep_dir)
                .expect("We should be able to find a repo root for in sigil subdir"),
            shallow_dir,
        );

        // Creating a file half way down shouldn't get mistaken for the sigil
        File::create(mid_dir.join(&dot_sigil)).expect("Failed to make testing file");
        assert_eq!(
            find_repo_root(&deep_dir)
                .expect("We should be able to find a repo root for in sigil subdir"),
            shallow_dir,
        );

        // This is optional but it gives us a way to see if the delete failed.
        tmp_dir.close().expect("Failed to delete tmp directory");
    }

    #[test]
    fn test_hg_repo_root() {
        test_git_hg_common("hg")
    }

    #[test]
    fn test_git_repo_root() {
        test_git_hg_common("git")
    }

    #[test]
    fn test_git_hg_interop() {
        let tmp_dir = make_tmp_dir().expect("Failed to create tmp dir for test");
        let path = tmp_dir.path();

        let shallow_dir = path.join("i/am");
        let mid_dir = shallow_dir.join("a/subdir");
        let deep_dir = mid_dir.join("of/the/repo");
        create_dir_all(&deep_dir).expect("Failed to make deep directory");

        create_dir(shallow_dir.join(".hg")).expect("failed to create .hg in shallow_dir");
        assert_eq!(
            find_repo_root(&shallow_dir)
                .expect("We should be able to find a repo root for in .hg subdir"),
            shallow_dir,
        );

        // Finally we should test to confirm that higher up .hg files take priority over lower .git
        // ones to match the bug in artifacts_dir python impl.
        create_dir(deep_dir.join(".git")).expect("failed to create .git in deep_dir");
        assert_eq!(
            find_repo_root(&deep_dir)
                .expect("We should be able to find a repo root for in hg subdir"),
            shallow_dir,
        );

        // This is optional but it gives us a way to see if the delete failed.
        tmp_dir.close().expect("Failed to delete tmp directory");
    }
}
