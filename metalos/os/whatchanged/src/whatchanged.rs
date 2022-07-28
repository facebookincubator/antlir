// (c) Facebook, Inc. and its affiliates. Confidential and proprietary.
#![deny(warnings)]
#![feature(map_first_last)]
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::fmt;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use anyhow::Context;
use anyhow::Result;
use btrfs::Subvolume;
use clap::Parser;

/// Show what happened to a subvolume since it was snapshotted.
#[derive(Parser)]
struct Opts {
    #[clap(default_value = "/")]
    subvol: PathBuf,
    #[clap(long, help = "path to pre-mounted parent subvolume")]
    parent: Option<PathBuf>,
}

/// Find all the files that have been modified in this subvolume since it was
/// created. Most of this will have been done by chef, but we can post-process
/// for the few cases where MetalOS expects file changes at runtime.
fn find_all_modified(subvol: &Subvolume) -> Result<BTreeSet<PathBuf>> {
    let create_generation = subvol.info().otransid;
    // TODO(T103370694): use BTRFS_IOC_TREE_SEARCH_V2 instead of shelling out
    let stdout = String::from_utf8(
        Command::new("btrfs")
            .arg("subvolume")
            .arg("find-new")
            .arg(subvol.path())
            .arg(create_generation.to_string())
            .output()?
            .stdout,
    )
    .context("btrfs subvolume find-new not utf8")?;
    Ok(stdout
        .lines()
        // the changed file path is the 16th column in `btrfs subvolume find-new` output
        .filter_map(|line| line.split_whitespace().nth(16))
        .map(|path| path.into())
        .collect())
}

/// Lots of files may appear as changed if some new RPMs are simply installed.
/// Technically we should probably verify that the contents have not changed,
/// but for the most part that would only be important for files in /etc which
/// we can special-case later. In the meantime, we can reasonably ignore files
/// that are owned by RPMs.
fn files_provided_by_rpms(root: &Path) -> Result<BTreeSet<PathBuf>> {
    let dbpath = root.join("var/lib/rpm");
    let stdout = String::from_utf8(
        Command::new("rpm")
            .arg("--dbpath")
            .arg(dbpath)
            .arg("--query")
            .arg("--all")
            .arg("--list")
            .output()
            .context("rpm query failed")?
            .stdout,
    )
    .context("rpm output not utf8")?;
    Ok(stdout.lines().map(|line| line.into()).collect())
}

/// Build a map of RPM name -> set of versions
fn installed_rpms(root: &Path) -> Result<BTreeMap<String, BTreeSet<String>>> {
    let dbpath = root.join("var/lib/rpm");
    let stdout = String::from_utf8(
        Command::new("rpm")
            .arg("--dbpath")
            .arg(dbpath)
            .arg("--query")
            .arg("--all")
            .arg("--queryformat")
            .arg("%{RPMTAG_NAME} %{RPMTAG_VERSION}-%{RPMTAG_RELEASE}\n")
            .output()
            .context("rpm query failed")?
            .stdout,
    )
    .context("rpm output not utf8")?;
    let mut map: BTreeMap<_, BTreeSet<_>> = BTreeMap::new();
    stdout
        .lines()
        .filter_map(|line| line.rsplit_once(' '))
        .for_each(|(name, ver)| {
            map.entry(name.to_owned())
                .or_default()
                .insert(ver.to_owned());
        });
    Ok(map)
}

fn get_changed_files(subvol: &Subvolume) -> Result<BTreeSet<PathBuf>> {
    let rpm_owned_files = files_provided_by_rpms(subvol.path())
        .context("failed to find files that were installed by rpms")?;
    Ok(find_all_modified(subvol)?
        .into_iter()
        .map(|path| Path::new("/").join(path))
        .filter(|path| !(path.starts_with("/var/cache") || path.starts_with("/var/log")))
        .filter(|path| !rpm_owned_files.contains(path))
        .filter(|path| {
            // if we failed to check the metalos.generator xattr, assume that
            // the file came from somewhere else and include it in the changed
            // files output
            match xattr::get(path, "user.metalos.generator") {
                Ok(Some(_)) => false,
                _ => true,
            }
        })
        .collect())
}

#[derive(Debug, PartialOrd, PartialEq, Ord, Eq)]
struct Rpm {
    name: String,
    version: String,
}

#[derive(Debug, PartialOrd, PartialEq, Ord, Eq)]
enum RpmDiff {
    Removed(Rpm),
    Installed(Rpm),
    Replaced(Rpm, Rpm),
}

impl fmt::Display for RpmDiff {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Removed(rpm) => {
                write!(f, "{} removed {}", rpm.name, rpm.version)
            }
            Self::Installed(rpm) => {
                write!(f, "{} installed {}", rpm.name, rpm.version)
            }
            Self::Replaced(old, new) => {
                write!(
                    f,
                    "{} replaced {} with {}",
                    old.name, old.version, new.version
                )
            }
        }
    }
}

fn find_rpm_differences(current: &Path, image: &Path) -> Result<BTreeSet<RpmDiff>> {
    let current_rpms =
        installed_rpms(current).context("failed to get installed rpms in root subvol")?;
    let mut image_rpms =
        installed_rpms(image).context("failed to get installed rpms in image subvol")?;

    let mut diffs = BTreeSet::new();
    for (name, mut versions) in current_rpms {
        if let Some(mut image_versions) = image_rpms.remove(&name) {
            if versions == image_versions {
                continue;
            }
            match (image_versions.len(), versions.len()) {
                // we can nicely explain a 1->1 version replacement
                (1, 1) => {
                    diffs.insert(RpmDiff::Replaced(
                        Rpm {
                            name: name.clone(),
                            version: image_versions
                                .pop_first()
                                .expect("have exactly 1 rpm version from image"),
                        },
                        Rpm {
                            name: name.clone(),
                            version: versions
                                .pop_first()
                                .expect("have exactly 1 rpm version from root"),
                        },
                    ));
                }
                // arbitrary number of versions is harder to cleanly explain, so
                // emit a Installed and Removed RpmDiff for each
                (_, _) => {
                    diffs.extend(versions.difference(&image_versions).map(|v| {
                        RpmDiff::Installed(Rpm {
                            name: name.clone(),
                            version: v.clone(),
                        })
                    }));
                    diffs.extend(image_versions.difference(&versions).map(|v| {
                        RpmDiff::Removed(Rpm {
                            name: name.clone(),
                            version: v.clone(),
                        })
                    }));
                }
            };
        } else {
            diffs.extend(versions.into_iter().map(|v| {
                RpmDiff::Installed(Rpm {
                    name: name.clone(),
                    version: v,
                })
            }));
        };
    }
    // all rpms left in the image_rpms map have been deleted since the image was
    // snapshotted
    for (name, versions) in image_rpms {
        diffs.extend(versions.into_iter().map(|v| {
            RpmDiff::Removed(Rpm {
                name: name.clone(),
                version: v,
            })
        }));
    }

    Ok(diffs)
}

fn main() -> Result<()> {
    let opts = Opts::parse();
    let subvol = Subvolume::get(opts.subvol)?;

    let parent = match opts.parent {
        Some(p) => p,
        None => {
            let parent_uuid = subvol
                .info()
                .parent_uuid
                .context("subvol has no parent id")?;
            let source = Subvolume::get(metalos_paths::control())?
                .children()?
                .filter_map(Result::ok)
                .find(|subvol| subvol.info().uuid == parent_uuid)
                .context("could not find source subvol")?;
            source.path().to_path_buf()
        }
    };

    println!("files changed:");
    get_changed_files(&subvol)?
        .iter()
        .for_each(|path| println!("  {}", path.display()));
    println!("rpms changed:");
    find_rpm_differences(subvol.path(), &parent)
        .context("failed to get rpm version differences")?
        .iter()
        .for_each(|diff| println!("  {}", diff));

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::find_rpm_differences;
    use super::get_changed_files;
    use super::Rpm;
    use super::RpmDiff;
    use anyhow::Result;
    use btrfs::Subvolume;
    use maplit::btreeset;
    use metalos_macros::containertest;
    use std::path::Path;

    #[containertest]
    fn test_file_change() -> Result<()> {
        let subvol = Subvolume::root()?;
        let cf = get_changed_files(&subvol)?;
        assert!(cf.is_empty(), "{:?}", cf);
        std::fs::write("/hello", "world!")?;
        let cf = get_changed_files(&subvol)?;
        assert_eq!(cf, btreeset! {"/hello".into()});
        Ok(())
    }

    #[containertest]
    fn test_rpm_changes() -> Result<()> {
        let cr = find_rpm_differences(Path::new("/"), Path::new("/mnt/parent"))?;
        assert_eq!(
            cr,
            btreeset! {
                RpmDiff::Installed(Rpm{name: "rpm-test-mice".into(), version: "0.1-a".into()}),
                RpmDiff::Removed(Rpm{name: "rpm-test-veggie".into(), version: "2-rc0".into()}),
                RpmDiff::Replaced(
                    Rpm{name: "rpm-test-cheese".into(), version: "1-1".into()},
                    Rpm{name: "rpm-test-cheese".into(), version: "2-1".into()},
                )
            }
        );
        Ok(())
    }
}
