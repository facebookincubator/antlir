/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Cheaply prove that a file in a snapshot has the same contents as the file
//! it was snapshotted from, without reading either of them.

use std::fs::File;
use std::mem::size_of;
use std::os::fd::AsRawFd;
use std::os::unix::fs::FileExt;
use std::path::Path;
use std::sync::atomic::Ordering;

use antlir2_btrfs::INO_SUBVOL;
use antlir2_btrfs::Info;
use cap_std::fs::Dir;
use cap_std::fs::Metadata;
use cap_std::fs::MetadataExt;
use nix::sys::ioctl::ioctl_num_type;
use nix::sys::statfs::BTRFS_SUPER_MAGIC;
use nix::sys::statfs::fstatfs;

use crate::Stats;

const VERIFY_CHUNK: usize = 1 << 20;

const FS_IOC_FIEMAP: ioctl_num_type =
    nix::request_code_readwrite!(b'f', 11, size_of::<FiemapHeader>());

/// Flush dirty data first, so that it has a physical location.
const FIEMAP_FLAG_SYNC: u32 = 0x0000_0001;

const FIEMAP_EXTENT_LAST: u32 = 0x0000_0001;
/// Also set for DELALLOC extents.
const FIEMAP_EXTENT_UNKNOWN: u32 = 0x0000_0002;
/// Also set for DATA_INLINE and DATA_TAIL extents.
const FIEMAP_EXTENT_NOT_ALIGNED: u32 = 0x0000_0100;

/// Extents with these flags do not have a stable physical location that
/// identifies their data.
const UNRELIABLE_FLAGS: u32 = FIEMAP_EXTENT_UNKNOWN | FIEMAP_EXTENT_NOT_ALIGNED;

const EXTENTS_PER_CALL: usize = 256;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct FiemapExtent {
    fe_logical: u64,
    fe_physical: u64,
    fe_length: u64,
    fe_reserved64: [u64; 2],
    fe_flags: u32,
    fe_reserved: [u32; 3],
}

/// `struct fiemap` without its flexible array of extents.
#[repr(C)]
struct FiemapHeader {
    fm_start: u64,
    fm_length: u64,
    fm_flags: u32,
    fm_mapped_extents: u32,
    fm_extent_count: u32,
    fm_reserved: u32,
}

#[repr(C)]
struct Fiemap {
    header: FiemapHeader,
    extents: [FiemapExtent; EXTENTS_PER_CALL],
}

// struct fiemap_extent from linux/fiemap.h: 3 u64, 2 u64 reserved, 1 u32, 3 u32 reserved.
const _: () = assert!(size_of::<FiemapExtent>() == 56);

nix::ioctl_readwrite_bad!(fiemap, FS_IOC_FIEMAP, Fiemap);

#[derive(Debug, thiserror::Error)]
pub(crate) enum NotSnapshot {
    #[error(transparent)]
    Btrfs(#[from] antlir2_btrfs::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("{0} is not on btrfs")]
    NotBtrfs(Side),
    #[error("{0} is not the root of a btrfs subvolume")]
    NotSubvolRoot(Side),
    #[error("new subvolume was not snapshotted from old subvolume")]
    NotParent,
    #[error("{0} subvolume is writable")]
    Writable(Side),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, derive_more::Display)]
pub(crate) enum Side {
    #[display("old")]
    Old,
    #[display("new")]
    New,
}

/// Proof that the new tree of a diff is a read-only btrfs snapshot of the
/// read-only old tree.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Snapshot {
    old_dev: u64,
    new_dev: u64,
}

impl Snapshot {
    /// Succeeds only if `new` is the root of a read-only btrfs subvolume that
    /// was snapshotted directly from the read-only subvolume at `old`.
    ///
    /// Everything is checked on the already-open directories, so the proof is
    /// about the trees that will actually be diffed. Read-only means neither
    /// tree can change while the diff runs.
    pub(crate) fn detect(old: &Dir, new: &Dir) -> Result<Self, NotSnapshot> {
        let (old_info, old_dev) = subvol_root(old, Side::Old)?;
        let (new_info, new_dev) = subvol_root(new, Side::New)?;
        if new_info.parent_uuid() != Some(old_info.uuid()) {
            return Err(NotSnapshot::NotParent);
        }
        if !old_info.is_readonly() {
            return Err(NotSnapshot::Writable(Side::Old));
        }
        if !new_info.is_readonly() {
            return Err(NotSnapshot::Writable(Side::New));
        }
        Ok(Self { old_dev, new_dev })
    }
}

/// Subvolume info and device of `dir`, which must be a subvolume root.
fn subvol_root(dir: &Dir, side: Side) -> Result<(Info, u64), NotSnapshot> {
    // cap_std opens directories with O_PATH, which ioctls reject with EBADF.
    // Reopening through /proc refers to the same directory, not a path lookup.
    let dir_fd = File::open(format!("/proc/self/fd/{}", dir.as_raw_fd()))?;
    if fstatfs(&dir_fd)
        .map_err(std::io::Error::from)?
        .filesystem_type()
        != BTRFS_SUPER_MAGIC
    {
        return Err(NotSnapshot::NotBtrfs(side));
    }
    let meta = dir.dir_metadata()?;
    if meta.ino() != INO_SUBVOL {
        return Err(NotSnapshot::NotSubvolRoot(side));
    }
    Ok((Info::from_fd(&dir_fd)?, meta.dev()))
}

/// Skips reading files that a [`Snapshot`] proves unchanged, optionally
/// byte-comparing them anyway. A file that fails verification is read in
/// full instead of failing the diff, so verification never breaks a build.
pub(crate) struct Shortcut {
    snapshot: Snapshot,
    verify_all: bool,
}

impl Shortcut {
    pub(crate) fn new(snapshot: Snapshot, verify_all: bool) -> Self {
        Self {
            snapshot,
            verify_all,
        }
    }

    /// Returns true if `new` does not need to be read because it is provably
    /// the same as `old`. A verified file that turns out to differ is read
    /// in full like any other changed file, and counted as mismatched, so
    /// the output is still correct and the build still passes.
    pub(crate) fn skip(
        &self,
        path: &Path,
        old: (&Metadata, &File),
        new: (&Metadata, &File),
        stats: &Stats,
    ) -> crate::Result<bool> {
        if !provably_unchanged(&self.snapshot, old.0, old.1, new.0, new.1) {
            return Ok(false);
        }

        if self.verify_all {
            stats.verified.fetch_add(1, Ordering::Relaxed);
            if !same_bytes(old.1, new.1, new.0.len())? {
                stats.mismatched.fetch_add(1, Ordering::Relaxed);
                tracing::error!(
                    path = %path.display(),
                    "fast snapshot diff mismatch: file differs even though its inode, ctime, size and extents match the snapshot; falling back to a full read",
                );
                return Ok(false);
            }
        }
        stats.skipped.fetch_add(1, Ordering::Relaxed);
        Ok(true)
    }
}

fn same_bytes(old: &File, new: &File, len: u64) -> std::io::Result<bool> {
    let chunk = len.min(VERIFY_CHUNK as u64) as usize;
    let mut old_buf = vec![0; chunk];
    let mut new_buf = vec![0; chunk];
    let mut offset = 0;
    while offset < len {
        let n = usize::try_from(len - offset).map_or(chunk, |rest| rest.min(chunk));
        old.read_exact_at(&mut old_buf[..n], offset)?;
        new.read_exact_at(&mut new_buf[..n], offset)?;
        if old_buf[..n] != new_buf[..n] {
            return Ok(false);
        }
        offset += n as u64;
    }
    Ok(true)
}

/// Physical extent map of the file as (logical, physical, length, flags), or
/// None if it cannot be used to identify the file's data.
fn extents(f: &impl AsRawFd) -> Option<Vec<(u64, u64, u64, u32)>> {
    let mut out = Vec::new();
    let mut start = 0u64;
    loop {
        let mut req = Fiemap {
            header: FiemapHeader {
                fm_start: start,
                fm_length: u64::MAX - start,
                fm_flags: FIEMAP_FLAG_SYNC,
                fm_mapped_extents: 0,
                fm_extent_count: EXTENTS_PER_CALL as u32,
                fm_reserved: 0,
            },
            extents: [FiemapExtent::default(); EXTENTS_PER_CALL],
        };
        // SAFETY: req is a struct fiemap with room for fm_extent_count extents
        if unsafe { fiemap(f.as_raw_fd(), &mut req) }.is_err() {
            return None;
        }
        let mapped = &req.extents[..req.header.fm_mapped_extents as usize];
        // An empty map means the file is empty or entirely sparse, and there
        // is no physical location to compare either way.
        let Some(last) = mapped.last() else {
            return (!out.is_empty()).then_some(out);
        };
        if mapped.iter().any(|e| e.fe_flags & UNRELIABLE_FLAGS != 0) {
            return None;
        }
        out.extend(
            mapped
                .iter()
                .map(|e| (e.fe_logical, e.fe_physical, e.fe_length, e.fe_flags)),
        );
        if last.fe_flags & FIEMAP_EXTENT_LAST != 0 {
            return Some(out);
        }
        match last.fe_logical.checked_add(last.fe_length) {
            Some(next) if next > start => start = next,
            _ => return None,
        }
    }
}

/// Returns true only if `old` and `new` are provably the same data, which is
/// the case for a file that was untouched after `snapshot` was taken.
///
/// Neither check stands alone. Shared physical extents are not sufficient:
/// btrfs reports compressed extents without the offset into the extent, so
/// two files referencing different ranges of one compressed extent can have
/// identical maps. Inode, size and ctime are not sufficient either: ctime is
/// a timestamp, not a change counter, so a write in the same clock tick could
/// in principle leave it unchanged. Together they close both gaps: a snapshot
/// preserves the inode number and ctime, any write or reflink into the file
/// sets ctime to the current time, and any write also copies the affected
/// shared extents, changing the map.
fn provably_unchanged(
    snapshot: &Snapshot,
    old_meta: &Metadata,
    old: &impl AsRawFd,
    new_meta: &Metadata,
    new: &impl AsRawFd,
) -> bool {
    // A file on a different device is in a nested subvolume, which the
    // snapshot did not copy.
    if old_meta.dev() != snapshot.old_dev || new_meta.dev() != snapshot.new_dev {
        return false;
    }
    let key = |m: &Metadata| (m.ino(), m.len(), m.ctime(), m.ctime_nsec());
    if key(old_meta) != key(new_meta) {
        return false;
    }
    let Some(old) = extents(old) else {
        return false;
    };
    extents(new).is_some_and(|new| new == old)
}

#[cfg(test)]
mod tests {
    use std::io::Seek;
    use std::io::SeekFrom;
    use std::io::Write;
    use std::path::PathBuf;

    use antlir2_btrfs::SnapshotFlags;
    use antlir2_btrfs::Subvolume;
    use cap_std::fs::OpenOptions;

    use super::*;

    const BLOCK: u64 = 4096;
    const MIB: usize = 1 << 20;

    nix::ioctl_write_int!(ficlone, 0x94, 9);

    struct Snapshotted {
        tmp: tempfile::TempDir,
        old_path: PathBuf,
        old: Dir,
        new_path: PathBuf,
        new: Dir,
    }

    impl Snapshotted {
        /// Creates a subvolume, lets `populate` write into it, snapshots it,
        /// and makes both read-only, the way antlir2 leaves image phases.
        fn new(populate: impl FnOnce(&Dir)) -> Self {
            Self::with_changes(populate, |_| {})
        }

        /// Like `new`, but lets `change` write into the snapshot before it is
        /// made read-only.
        fn with_changes(populate: impl FnOnce(&Dir), change: impl FnOnce(&Dir)) -> Self {
            let s = Self::writable(populate);
            change(&s.new);
            for path in [&s.old_path, &s.new_path] {
                Subvolume::open(path)
                    .expect("open subvol")
                    .set_readonly(true)
                    .expect("set readonly");
            }
            s
        }

        /// A snapshot where both subvolumes are left writable.
        fn writable(populate: impl FnOnce(&Dir)) -> Self {
            let tmp = tempfile::tempdir_in("/work").expect("tempdir");
            let old_path = tmp.path().join("old");
            let new_path = tmp.path().join("new");
            let old_subvol = Subvolume::create(&old_path).expect("create subvol");
            let old = open_dir(&old_path);
            populate(&old);
            old_subvol
                .snapshot(&new_path, SnapshotFlags::empty())
                .expect("snapshot");
            let new = open_dir(&new_path);
            Self {
                tmp,
                old_path,
                old,
                new_path,
                new,
            }
        }

        fn detect(&self) -> Result<Snapshot, NotSnapshot> {
            Snapshot::detect(&self.old, &self.new)
        }

        fn unchanged(&self, old_name: &str, new_name: &str) -> bool {
            let snapshot = self.detect().expect("detect");
            let (old_meta, old) = open(&self.old, old_name);
            let (new_meta, new) = open(&self.new, new_name);
            provably_unchanged(&snapshot, &old_meta, &old, &new_meta, &new)
        }

        fn skip(&self, shortcut: &Shortcut, name: &str, stats: &Stats) -> crate::Result<bool> {
            let (old_meta, old) = open(&self.old, name);
            let (new_meta, new) = open(&self.new, name);
            shortcut.skip(Path::new(name), (&old_meta, &old), (&new_meta, &new), stats)
        }
    }

    fn open_dir(path: &Path) -> Dir {
        Dir::open_ambient_dir(path, cap_std::ambient_authority()).expect("open dir")
    }

    fn tempdir() -> (tempfile::TempDir, Dir) {
        let tmp = tempfile::tempdir_in("/work").expect("tempdir");
        let dir = open_dir(tmp.path());
        (tmp, dir)
    }

    fn write_at(dir: &Dir, name: &str, offset: u64, data: &[u8]) {
        let mut f = dir
            .open_with(name, OpenOptions::new().create(true).write(true))
            .expect("open for write");
        f.seek(SeekFrom::Start(offset)).expect("seek");
        f.write_all(data).expect("write");
        f.sync_all().expect("sync");
    }

    fn open(dir: &Dir, name: &str) -> (Metadata, std::fs::File) {
        let f = dir.open(name).expect("open").into_std();
        let meta = dir.metadata(name).expect("metadata");
        (meta, f)
    }

    #[test]
    fn fiemap_request_code_matches_kernel() {
        assert_eq!(
            FS_IOC_FIEMAP, 0xC020_660B,
            "_IOWR('f', 11, struct fiemap) from linux/fs.h"
        );
    }

    #[test]
    fn detects_read_only_snapshot() {
        let s = Snapshotted::new(|_| {});
        let snapshot = s.detect().expect("new was snapshotted from old");
        assert_eq!(snapshot.old_dev, s.old.dir_metadata().expect("stat").dev());
        assert_eq!(snapshot.new_dev, s.new.dir_metadata().expect("stat").dev());
    }

    #[test]
    fn rejects_writable_subvolumes() {
        let s = Snapshotted::writable(|_| {});
        assert!(
            matches!(s.detect(), Err(NotSnapshot::Writable(Side::Old))),
            "a writable old tree could change during the diff"
        );
        Subvolume::open(&s.old_path)
            .expect("open subvol")
            .set_readonly(true)
            .expect("set readonly");
        assert!(
            matches!(s.detect(), Err(NotSnapshot::Writable(Side::New))),
            "a writable new tree could change during the diff"
        );
    }

    #[test]
    fn rejects_trees_that_are_not_a_direct_snapshot() {
        let s = Snapshotted::new(|_| {});
        assert!(
            matches!(
                Snapshot::detect(&s.new, &s.old),
                Err(NotSnapshot::NotParent)
            ),
            "old is the parent of new, not the other way around"
        );

        let unrelated_path = s.tmp.path().join("unrelated");
        Subvolume::create(&unrelated_path).expect("create subvol");
        let unrelated = open_dir(&unrelated_path);
        assert!(
            matches!(
                Snapshot::detect(&s.old, &unrelated),
                Err(NotSnapshot::NotParent)
            ),
            "a fresh subvolume has no parent"
        );

        let (_tmp, plain) = tempdir();
        plain.create_dir("dir").expect("mkdir");
        let dir = plain.open_dir("dir").expect("open dir");
        assert!(
            matches!(
                Snapshot::detect(&s.old, &dir),
                Err(NotSnapshot::NotSubvolRoot(Side::New))
            ),
            "a plain directory is not a snapshot"
        );
    }

    #[test]
    fn untouched_file_is_unchanged() {
        let s = Snapshotted::new(|d| write_at(d, "a", 0, &[b'x'; MIB]));
        assert!(
            s.unchanged("a", "a"),
            "a file untouched since the snapshot must be provably unchanged"
        );
    }

    #[test]
    fn same_size_overwrite_is_not_unchanged() {
        let s = Snapshotted::with_changes(
            |d| write_at(d, "a", 0, &[b'x'; MIB]),
            |d| write_at(d, "a", MIB as u64 / 2, &[b'y'; BLOCK as usize]),
        );

        let (old_meta, old) = open(&s.old, "a");
        let (new_meta, new) = open(&s.new, "a");
        assert_eq!(old_meta.ino(), new_meta.ino());
        assert_eq!(old_meta.len(), new_meta.len());
        assert_ne!(
            extents(&old),
            extents(&new),
            "a COW overwrite must change the extent map even when the size does not change"
        );
        assert!(!s.unchanged("a", "a"));
    }

    #[test]
    fn reflinked_copy_is_not_unchanged() {
        let s = Snapshotted::with_changes(
            |d| write_at(d, "a", 0, &[b'x'; MIB]),
            |d| {
                let (_, src) = open(d, "a");
                let dst = d.create("b").expect("create").into_std();
                // SAFETY: both are valid fds
                unsafe { ficlone(dst.as_raw_fd(), src.as_raw_fd() as _) }.expect("FICLONE");
            },
        );

        let (_, old) = open(&s.old, "a");
        let (_, new) = open(&s.new, "b");
        assert_eq!(
            extents(&old),
            extents(&new),
            "a reflinked copy shares every extent with its source"
        );
        assert!(
            !s.unchanged("a", "b"),
            "a different inode must never be provably unchanged, even if it shares extents"
        );
    }

    #[test]
    fn other_device_is_not_unchanged() {
        let s = Snapshotted::new(|d| write_at(d, "a", 0, &[b'x'; MIB]));
        let snapshot = s.detect().expect("detect");
        let snapshot = Snapshot {
            new_dev: snapshot.new_dev + 1,
            ..snapshot
        };
        let (old_meta, old) = open(&s.old, "a");
        let (new_meta, new) = open(&s.new, "a");
        assert!(
            !provably_unchanged(&snapshot, &old_meta, &old, &new_meta, &new),
            "a file outside the snapshotted subvolume must not be provably unchanged"
        );
    }

    #[test]
    fn shortcut_skips_and_verifies_untouched_files() {
        let s = Snapshotted::with_changes(
            |d| {
                write_at(d, "untouched", 0, &[b'x'; MIB]);
                write_at(d, "overwritten", 0, &[b'x'; MIB]);
            },
            |d| write_at(d, "overwritten", 0, &[b'y'; BLOCK as usize]),
        );
        let shortcut = Shortcut::new(s.detect().expect("detect"), true);
        let stats = Stats::new(true);

        assert!(
            s.skip(&shortcut, "untouched", &stats).expect("skip"),
            "an untouched file does not need to be read"
        );
        assert!(
            !s.skip(&shortcut, "overwritten", &stats).expect("skip"),
            "an overwritten file must be read"
        );
        assert_eq!(stats.skipped(), 1, "only the untouched file was skipped");
        assert_eq!(
            stats.verified(),
            1,
            "verify_all byte-compares every skipped file"
        );
        assert_eq!(
            stats.mismatched(),
            0,
            "no skipped file may differ from its snapshot"
        );
    }

    #[test]
    fn same_bytes_compares_every_chunk() {
        let (_tmp, dir) = tempdir();
        let len = VERIFY_CHUNK as u64 * 2 + 1;
        write_at(&dir, "a", 0, &vec![b'x'; len as usize]);
        write_at(&dir, "b", 0, &vec![b'x'; len as usize]);
        write_at(&dir, "c", 0, &vec![b'x'; len as usize]);
        write_at(&dir, "c", len - 1, b"y");
        let (_, a) = open(&dir, "a");
        let (_, b) = open(&dir, "b");
        let (_, c) = open(&dir, "c");
        assert!(same_bytes(&a, &b, len).expect("read"));
        assert!(
            !same_bytes(&a, &c, len).expect("read"),
            "a difference in the last byte, past the second chunk, must be found"
        );
    }

    #[test]
    fn extent_map_is_paged() {
        let (_tmp, dir) = tempdir();
        // holes between the blocks keep btrfs from merging them into fewer
        // extents than EXTENTS_PER_CALL, and nothing is written at offset 0
        // where a compressible block would be stored inline
        let blocks = EXTENTS_PER_CALL as u64 * 2 + 1;
        for i in 0..blocks {
            write_at(&dir, "a", (i * 2 + 1) * BLOCK, &[b'x'; BLOCK as usize]);
        }
        let (_, f) = open(&dir, "a");
        let map = extents(&f).expect("extents");
        assert_eq!(map.len() as u64, blocks, "one extent per written block");
        assert!(
            map.windows(2).all(|w| w[0].0 < w[1].0),
            "extents must be returned in order without repeats across pages"
        );
    }

    #[test]
    fn empty_and_sparse_files_have_no_extents() {
        let (_tmp, dir) = tempdir();
        dir.create("empty").expect("create");
        dir.create("sparse")
            .expect("create")
            .set_len(MIB as u64)
            .expect("set_len");
        for name in ["empty", "sparse"] {
            let (_, f) = open(&dir, name);
            assert_eq!(extents(&f), None, "{name} has no data to identify");
        }
    }
}
