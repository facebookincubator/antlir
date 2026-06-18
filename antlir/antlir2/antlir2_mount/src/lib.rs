/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::Path;
use std::path::PathBuf;

use nix::mount::MntFlags;
use nix::mount::MsFlags;
use proc_mounts::MountIter;
use tracing::info;

#[derive(thiserror::Error, Debug)]
pub enum MountError {
    #[error("No such file or directory: Mount source {0:?} doesn't exist")]
    MissingSource(PathBuf),
    #[error("No such file or directory: Mount target {0:?} doesn't exist")]
    MissingTarget(PathBuf),
    #[error("No such file or directory: Unknown reason - both source/target exist")]
    MissingUnknown,
    #[error("Path {0:?} provided as subvolume path is not valid unicode")]
    InvalidSubvolume(PathBuf),
    #[error("Cannot parse /proc/mounts: {0:?}")]
    ParseError(std::io::Error),
    #[error("Cannot canonicalize mount source {path:?}: {error}")]
    CanonicalizeMountSource {
        path: PathBuf,
        error: std::io::Error,
    },
    #[error(
        "Mount target {target:?} is already mounted from {mounted_source:?}, expected source {expected_source:?}"
    )]
    AlreadyMounted {
        target: PathBuf,
        mounted_source: PathBuf,
        expected_source: PathBuf,
    },
    #[error("Unknown error occurred: {0:?}")]
    Unknown(#[from] nix::errno::Errno),
}

// We use this instead of proc_mounts::source_mounted_at to ignore possible iteration errors
pub fn source_mounted_at(source: &Path, target: &Path) -> Result<bool, MountError> {
    for mount in MountIter::new().map_err(MountError::ParseError)? {
        if let Ok(mount_info) = mount {
            if mount_info.dest == target {
                if same_source_for_mount_reuse(&mount_info.source, source)? {
                    return Ok(true);
                }
            }
        }
    }
    Ok(false)
}

fn mounted_source_at(target: &Path) -> Result<Option<PathBuf>, MountError> {
    Ok(MountIter::new()
        .map_err(MountError::ParseError)?
        .filter_map(|m| m.ok())
        .filter(|m| m.dest == target)
        .map(|m| m.source)
        .last())
}

fn canonicalize_mount_source(source: &Path) -> Result<PathBuf, MountError> {
    source
        .canonicalize()
        .map_err(|error| MountError::CanonicalizeMountSource {
            path: source.to_path_buf(),
            error,
        })
}

fn same_source(a: &Path, b: &Path) -> Result<bool, MountError> {
    if a == b {
        return Ok(true);
    }

    // Pseudo-fs sources ("tmpfs", "proc", "none", etc.) are not absolute paths
    // and cannot be canonicalized. Since byte equality was already checked above,
    // two different non-absolute sources are by definition different.
    if !a.is_absolute() || !b.is_absolute() {
        return Ok(false);
    }

    Ok(canonicalize_mount_source(a)? == canonicalize_mount_source(b)?)
}

fn same_source_for_mount_reuse(
    mounted_source: &Path,
    expected_source: &Path,
) -> Result<bool, MountError> {
    match same_source(mounted_source, expected_source) {
        Ok(matches) => Ok(matches),
        // A stale/non-existent mount source cannot be canonicalized. Treat it
        // as a mismatch so callers can keep their mount-table error handling.
        Err(MountError::CanonicalizeMountSource { .. }) => Ok(false),
        Err(e) => Err(e),
    }
}

#[mockall::automock]
pub trait Mounter: Sized {
    fn mount<'a, 'b>(
        &'a self,
        source: &'b Path,
        target: &'b Path,
        fstype: Option<&'b str>,
        flags: MsFlags,
        data: Option<&'b str>,
    ) -> Result<MountHandle<'a, Self>, MountError>;

    fn umount(&self, mountpoint: &Path, force: bool) -> Result<(), nix::errno::Errno>;
}

// RealMounter is an implementation of the Mounter trait that calls nix::mount::mount for real.
pub struct RealMounter;

impl Mounter for RealMounter {
    fn mount<'a, 'b>(
        &'a self,
        source: &'b Path,
        target: &'b Path,
        fstype: Option<&'b str>,
        flags: MsFlags,
        data: Option<&'b str>,
    ) -> Result<MountHandle<'a, Self>, MountError> {
        info!(
            "Mounting {} to {} with fstype {:?}, flags {:?} and options {:?}",
            source.display(),
            target.display(),
            fstype,
            flags,
            data
        );
        match nix::mount::mount(Some(source), target, fstype, flags, data) {
            Ok(()) => Ok(MountHandle::new(target.to_path_buf(), self)),
            Err(nix::errno::Errno::ENOENT) => Err(if !target.exists() {
                MountError::MissingTarget(target.to_path_buf())
            } else if !source.exists() {
                MountError::MissingSource(source.to_path_buf())
            } else {
                MountError::MissingUnknown
            }),
            Err(nix::errno::Errno::EBUSY) => match mounted_source_at(target)? {
                Some(mounted_source) => {
                    if same_source_for_mount_reuse(&mounted_source, source)? {
                        info!(
                            "Mount {} to {} already exists, reusing it",
                            source.display(),
                            target.display()
                        );
                        Ok(MountHandle::existing(target.to_path_buf(), self))
                    } else {
                        Err(MountError::AlreadyMounted {
                            target: target.to_path_buf(),
                            mounted_source,
                            expected_source: source.to_path_buf(),
                        })
                    }
                }
                None => Err(nix::errno::Errno::EBUSY.into()),
            },
            Err(e) => Err(e.into()),
        }
    }

    fn umount(&self, mountpoint: &Path, force: bool) -> Result<(), nix::errno::Errno> {
        let mut flags = MntFlags::empty();
        if force {
            flags.insert(MntFlags::MNT_FORCE);
        }
        info!("Unmounting {} with flags {:?}", mountpoint.display(), flags);
        nix::mount::umount2(mountpoint, flags)
    }
}

/// This mounter is bounded to live at most as long as the
/// mounter that it contains and will give out auto-unmounting
/// mounts. The primary use for this is to have mounts that aren't
/// meant to survive longer than something local to the binary.
/// For example if a loopback device is created and things are mounted from it
/// this can be used to ensure that the mounts are taken down before the loopback
/// device is detatched
pub struct BoundMounter<'a, M: Mounter>(&'a M);

impl<'a, M> BoundMounter<'a, M>
where
    M: Mounter,
{
    pub fn new(binding_reference: &'a M) -> Self {
        Self(binding_reference)
    }
}

impl<'limit, M> Mounter for BoundMounter<'limit, M>
where
    M: Mounter,
{
    fn mount<'a, 'b>(
        &'a self,
        source: &'b Path,
        target: &'b Path,
        fstype: Option<&'b str>,
        flags: MsFlags,
        data: Option<&'b str>,
    ) -> Result<MountHandle<'a, Self>, MountError> {
        match self.0.mount(source, target, fstype, flags, data) {
            Ok(mut handle) => {
                handle.auto_umount();
                Ok(handle.replace_mounter_unchecked(self))
            }
            Err(e) => Err(e),
        }
    }

    fn umount(&self, mountpoint: &Path, force: bool) -> Result<(), nix::errno::Errno> {
        self.0.umount(mountpoint, force)
    }
}

pub struct MountHandle<'a, M>
where
    M: Mounter,
{
    target: PathBuf,
    mounter: &'a M,
    auto_umount: bool,
    owns_mount: bool,
    unmounted: bool,
}

impl<'a, M> MountHandle<'a, M>
where
    M: Mounter,
{
    fn new(target: PathBuf, mounter: &'a M) -> Self {
        Self {
            target,
            mounter,
            auto_umount: false,
            owns_mount: true,
            unmounted: false,
        }
    }

    fn existing(target: PathBuf, mounter: &'a M) -> Self {
        Self {
            target,
            mounter,
            auto_umount: false,
            owns_mount: false,
            unmounted: false,
        }
    }

    pub fn umount(mut self, force: bool) -> Result<(), nix::errno::Errno> {
        if self.owns_mount {
            self.mounter.umount(&self.target, force)?;
        }
        self.unmounted = true;
        Ok(())
    }

    fn replace_mounter_unchecked<'b, NM: Mounter>(
        mut self,
        new_mounter: &'b NM,
    ) -> MountHandle<'b, NM> {
        let auto_umount = self.auto_umount;
        self.auto_umount = false;
        MountHandle {
            target: self.target.clone(),
            mounter: new_mounter,
            auto_umount,
            owns_mount: self.owns_mount,
            unmounted: false,
        }
    }

    pub fn auto_umount(&mut self) {
        self.auto_umount = true;
    }

    pub fn mountpoint(&self) -> &Path {
        &self.target
    }

    pub fn mount_relative<'b, 'c>(
        &'a self,
        source: &'c Path,
        relative_target: &'c Path,
        fstype: Option<&'c str>,
        flags: MsFlags,
        data: Option<&'c str>,
    ) -> Result<MountHandle<'b, M>, MountError>
    where
        'a: 'b,
    {
        let target = self.target.join(relative_target);
        let mut handle = self.mounter.mount(source, &target, fstype, flags, data)?;

        if self.auto_umount {
            handle.auto_umount();
        }
        Ok(handle)
    }
}

impl<'a, M> Drop for MountHandle<'a, M>
where
    M: Mounter,
{
    fn drop(&mut self) {
        if self.auto_umount && self.owns_mount && !self.unmounted {
            let _ = self.mounter.umount(&self.target, true);
            self.unmounted = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    struct TestMounter {
        mount_creates_new_mount: bool,
        umounts: Cell<usize>,
    }

    impl TestMounter {
        fn new(mount_creates_new_mount: bool) -> Self {
            Self {
                mount_creates_new_mount,
                umounts: Cell::new(0),
            }
        }
    }

    impl Mounter for TestMounter {
        fn mount<'a, 'b>(
            &'a self,
            _source: &'b Path,
            target: &'b Path,
            _fstype: Option<&'b str>,
            _flags: MsFlags,
            _data: Option<&'b str>,
        ) -> Result<MountHandle<'a, Self>, MountError> {
            if self.mount_creates_new_mount {
                Ok(MountHandle::new(target.to_path_buf(), self))
            } else {
                Ok(MountHandle::existing(target.to_path_buf(), self))
            }
        }

        fn umount(&self, _mountpoint: &Path, _force: bool) -> Result<(), nix::errno::Errno> {
            self.umounts.set(self.umounts.get() + 1);
            Ok(())
        }
    }

    #[test]
    fn auto_umount_unmounts_owned_mount_on_drop() {
        let mounter = TestMounter::new(true);
        let mut handle = MountHandle::new(PathBuf::from("/target"), &mounter);
        handle.auto_umount();

        drop(handle);

        assert_eq!(mounter.umounts.get(), 1);
    }

    #[test]
    fn auto_umount_does_not_unmount_existing_mount_on_drop() {
        let mounter = TestMounter::new(true);
        let mut handle = MountHandle::existing(PathBuf::from("/target"), &mounter);
        handle.auto_umount();

        drop(handle);

        assert_eq!(mounter.umounts.get(), 0);
    }

    #[test]
    fn explicit_umount_does_not_unmount_existing_mount() {
        let mounter = TestMounter::new(true);
        let handle = MountHandle::existing(PathBuf::from("/target"), &mounter);

        handle
            .umount(false)
            .expect("existing mount handle should be treated as already unmounted");

        assert_eq!(mounter.umounts.get(), 0);
    }

    #[test]
    fn same_source_treats_equal_raw_paths_as_same_without_canonicalizing() {
        let missing_path = Path::new("/__antlir_mount_test_missing_source__");

        assert!(
            same_source(missing_path, missing_path)
                .expect("equal raw paths should not require canonicalization")
        );
    }

    #[test]
    fn same_source_propagates_canonicalize_errors() {
        let error = same_source(
            Path::new("/__antlir_mount_test_missing_source_a__"),
            Path::new("/__antlir_mount_test_missing_source_b__"),
        )
        .expect_err("different missing paths should fail canonicalization");

        assert!(
            matches!(error, MountError::CanonicalizeMountSource { .. }),
            "expected canonicalization error, got {error:?}"
        );
    }

    #[test]
    fn same_source_for_mount_reuse_treats_canonicalize_errors_as_mismatch() {
        assert!(
            !same_source_for_mount_reuse(
                Path::new("/__antlir_mount_test_missing_source_a__"),
                Path::new("/__antlir_mount_test_missing_source_b__"),
            )
            .expect("stale mount source should be treated as a mismatch"),
            "stale mount source should not be reused"
        );
    }

    #[test]
    fn same_source_treats_different_pseudo_fs_sources_as_different() {
        assert!(
            !same_source(Path::new("tmpfs"), Path::new("proc"))
                .expect("non-absolute pseudo-fs sources should not error"),
            "different pseudo-fs sources should be treated as different"
        );
    }

    #[test]
    fn bound_mounter_preserves_existing_mount_ownership() {
        let mounter = TestMounter::new(false);
        let bound_mounter = BoundMounter::new(&mounter);

        {
            let _handle = bound_mounter
                .mount(
                    Path::new("/source"),
                    Path::new("/target"),
                    None,
                    MsFlags::empty(),
                    None,
                )
                .expect("test mounter should return an existing mount handle");
        }

        assert_eq!(mounter.umounts.get(), 0);
    }
}
