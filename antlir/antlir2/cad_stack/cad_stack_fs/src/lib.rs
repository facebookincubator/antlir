/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::VecDeque;
use std::ffi::OsString;
use std::path::Path;
use std::path::PathBuf;

use cad_stack::Checksum;
use cad_stack::ObjectStore;
use cap_std::fs;
use cap_std::fs::DirEntry;
use cap_std::fs::MetadataExt;
use cap_std::fs::OpenOptions;
use cap_std::fs::OpenOptionsExt;
use cap_std::fs::PermissionsExt;
use cap_std::io_lifetimes::AsFd;
use rustix::fs::AtFlags;
use rustix::fs::Gid;
use rustix::fs::Uid;
use rustix::fs::chownat;
use rustix::fs::fchown;
use rustix::fs::readlinkat;
use rustix::fs::symlinkat;
use xattr::FileExt;

mod dir;
mod file_content;
mod meta;

use crate::dir::Dir;
use crate::dir::DirEntry as DirEntryObject;
use crate::file_content::FileContent;
use crate::meta::Data;
use crate::meta::Inode;

pub const ALIAS_ROOT: &str = "ROOT";

/// Describes what filesystem operation was being performed when an error occurred.
#[derive(Debug, Clone, Copy)]
pub enum Operation {
    CreateDir,
    CreateFile,
    CreateHardlink,
    CreateSymlink,
    GetFileType,
    ListDir,
    OpenDir,
    OpenFile,
    ReadLink,
    ReadMetadata,
    ReadXattrs,
    SetPermissions,
    SetOwnership,
    SetXattrs,
    WriteFile,
}

impl std::fmt::Display for Operation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CreateDir => write!(f, "creating directory"),
            Self::CreateFile => write!(f, "creating file"),
            Self::CreateHardlink => write!(f, "creating hardlink"),
            Self::CreateSymlink => write!(f, "creating symlink"),
            Self::GetFileType => write!(f, "getting file type"),
            Self::ListDir => write!(f, "listing directory"),
            Self::OpenDir => write!(f, "opening directory"),
            Self::OpenFile => write!(f, "opening file"),
            Self::ReadLink => write!(f, "reading symlink target"),
            Self::ReadMetadata => write!(f, "reading metadata"),
            Self::ReadXattrs => write!(f, "reading xattrs"),
            Self::SetPermissions => write!(f, "setting permissions"),
            Self::SetOwnership => write!(f, "setting ownership"),
            Self::SetXattrs => write!(f, "setting xattrs"),
            Self::WriteFile => write!(f, "writing file content"),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{op} '{path}': {source}")]
    Io {
        op: Operation,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error(transparent)]
    Store(#[from] cad_stack::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

trait IoContext<T> {
    fn io_context(self, op: Operation, path: &Path) -> Result<T>;
}

impl<T> IoContext<T> for std::io::Result<T> {
    fn io_context(self, op: Operation, path: &Path) -> Result<T> {
        self.map_err(|source| Error::Io {
            op,
            path: path.to_owned(),
            source,
        })
    }
}

impl<T> IoContext<T> for std::result::Result<T, rustix::io::Errno> {
    fn io_context(self, op: Operation, path: &Path) -> Result<T> {
        self.map_err(|e| Error::Io {
            op,
            path: path.to_owned(),
            source: e.into(),
        })
    }
}

/// Recursively add a directory tree to the object store.
///
/// This function traverses the entire directory tree rooted at `dir`,
/// storing all files, symlinks, and subdirectories in the object store.
/// Returns the checksum of the root directory object.
pub fn add_dir_recursive(store: &ObjectStore, dir: fs::Dir) -> Result<Checksum<Dir>> {
    let mut seen_inodes: HashMap<(u64, u64), (PathBuf, Checksum<Inode>)> = HashMap::new();
    add_dir_recursive_impl(store, dir, Path::new("."), &mut seen_inodes)
}

/// Same as [add_dir_recursive] but also mark this directory as "the root dir"
/// of the layer for easy lookup later
pub fn add_root_dir(store: &ObjectStore, dir: fs::Dir) -> Result<Checksum<Dir>> {
    let root_dir = add_dir_recursive(store, dir)?;
    store.set_alias(ALIAS_ROOT, &root_dir)?;
    Ok(root_dir)
}

fn add_dir_recursive_impl(
    store: &ObjectStore,
    dir: fs::Dir,
    current_path: &Path,
    seen_inodes: &mut HashMap<(u64, u64), (PathBuf, Checksum<Inode>)>,
) -> Result<Checksum<Dir>> {
    let (object, subdirs) = scan_dir(store, &dir, current_path, seen_inodes)?;
    let mut stack = vec![Frame {
        dir,
        path: current_path.to_owned(),
        object,
        pending_subdirs: subdirs,
    }];

    loop {
        // Try to descend into a pending subdirectory. The closure limits
        // the mutable borrow on `stack` so we can push/pop afterward.
        let descend = stack.last_mut().and_then(|frame| {
            frame
                .pending_subdirs
                .pop_front()
                .map(|(name, subdir_path)| {
                    let open_result = frame.dir.open_dir(&name);
                    (open_result, subdir_path)
                })
        });

        if let Some((open_result, subdir_path)) = descend {
            let subdir = open_result.io_context(Operation::OpenDir, &subdir_path)?;
            let (object, subdirs) = scan_dir(store, &subdir, &subdir_path, seen_inodes)?;
            stack.push(Frame {
                dir: subdir,
                path: subdir_path,
                object,
                pending_subdirs: subdirs,
            });
        } else {
            // All subdirectories have been processed; finalize this directory
            let Some(frame) = stack.pop() else {
                unreachable!("stack should not be empty before root is processed");
            };
            let checksum = store.store(&frame.object)?;

            if let Some(parent) = stack.last_mut() {
                let dir_name =
                    frame
                        .path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .ok_or_else(|| Error::Io {
                            op: Operation::OpenDir,
                            path: frame.path.clone(),
                            source: std::io::Error::new(
                                std::io::ErrorKind::InvalidData,
                                "invalid directory name",
                            ),
                        })?;
                parent.object.add_directory(dir_name.to_owned(), checksum);
            } else {
                return Ok(checksum);
            }
        }
    }
}

// Each frame on the explicit stack represents a directory being processed.
// The `dir` handle is kept open to lazily open subdirectories one at a
// time, keeping the number of open file descriptors proportional to the
// directory depth rather than the total number of directories.
struct Frame {
    dir: fs::Dir,
    path: PathBuf,
    object: Dir,
    pending_subdirs: VecDeque<(String, PathBuf)>,
}

// Scan a directory: process all non-directory entries immediately and
// collect subdirectory names for deferred processing.
fn scan_dir(
    store: &ObjectStore,
    dir: &fs::Dir,
    path: &Path,
    seen_inodes: &mut HashMap<(u64, u64), (PathBuf, Checksum<Inode>)>,
) -> Result<(Dir, VecDeque<(String, PathBuf)>)> {
    let mut object = Dir::builder()
        .meta(store.store(&get_dir_meta(dir, path)?)?)
        .build();

    // Collect and sort entries by name for deterministic hardlink inode
    // path naming
    let mut entries: Vec<DirEntry> = dir
        .entries()
        .io_context(Operation::ListDir, path)?
        .collect::<std::io::Result<Vec<_>>>()
        .io_context(Operation::ListDir, path)?;
    entries.sort_by_key(|e| e.file_name());

    let mut subdirs = VecDeque::new();

    for ent in entries {
        let name = ent.file_name().into_string().map_err(|os_str| Error::Io {
            op: Operation::ListDir,
            path: path.join(PathBuf::from(os_str)),
            source: std::io::Error::new(std::io::ErrorKind::InvalidData, "non-utf8 file name"),
        })?;
        let entry_path = path.join(&name);
        let ft = ent
            .file_type()
            .io_context(Operation::GetFileType, &entry_path)?;
        if ft.is_dir() {
            subdirs.push_back((name, entry_path));
        } else if ft.is_symlink() {
            let meta = get_symlink_meta(dir, &ent, &entry_path)?;
            let meta_checksum = store.store(&meta)?;
            object.add_file(name, meta_checksum);
        } else if ft.is_file() {
            let f = ent.open().io_context(Operation::OpenFile, &entry_path)?;
            let file_stat = f
                .metadata()
                .io_context(Operation::ReadMetadata, &entry_path)?;
            let dev_ino = (file_stat.dev(), file_stat.ino());
            if let Some((first_target, meta_checksum)) = seen_inodes.get(&dev_ino) {
                object.add_hardlink(name, first_target.clone(), *meta_checksum);
            } else {
                let meta = get_file_meta(store, f, &entry_path)?;
                let meta_checksum = store.store(&meta)?;
                object.add_file(name, meta_checksum);
                seen_inodes.insert(dev_ino, (entry_path, meta_checksum));
            }
        }
    }

    Ok((object, subdirs))
}

/// Extract a directory tree from the object store to a real filesystem
/// location.
///
/// This function recursively materializes all files, symlinks, and
/// subdirectories from the stored directory object to the given
/// target directory. File permissions, ownership (if possible), and
/// extended attributes are restored.
pub fn extract_dir_recursive(
    store: &ObjectStore,
    dir_checksum: &Checksum<Dir>,
    target: &fs::Dir,
) -> Result<()> {
    extract_dir_recursive_impl(store, dir_checksum, target, target, Path::new("."))
}

/// Same as [extract_dir_recursive] but looks up the root dir instead of needing
/// to be provided with a checksum
pub fn extract_root_dir(store: &ObjectStore, target: &fs::Dir) -> Result<()> {
    let root = store.get_alias_checksum(ALIAS_ROOT)?;
    extract_dir_recursive(store, &root, target)
}

fn extract_dir_recursive_impl(
    store: &ObjectStore,
    dir_checksum: &Checksum<Dir>,
    target: &fs::Dir,
    root_target: &fs::Dir,
    current_path: &Path,
) -> Result<()> {
    let dir = store.load(dir_checksum)?;

    // Apply directory metadata
    let dir_meta = store.load(dir.meta())?;
    apply_meta_to_dir(target, &dir_meta, current_path)?;

    // Process entries in three passes to match the ingestion order. During
    // ingestion, files at each directory level are processed before
    // subdirectories (and entries are sorted alphabetically), so the canonical
    // File for any hardlinked inode is always at the shallowest level or in an
    // earlier-alphabetical sibling directory. Creating all File entries first,
    // then Hardlinks, then recursing into Dirs guarantees that hardlink targets
    // always exist when needed.

    // Pass 1: Create all regular files and symlinks
    for (name, entry) in dir.entries() {
        if let DirEntryObject::File(inode_checksum) = entry {
            let entry_path = current_path.join(name);
            let inode = store.load(inode_checksum)?;

            if let Some(link_target) = inode.link_target() {
                symlinkat(link_target, target.as_fd(), name)
                    .io_context(Operation::CreateSymlink, &entry_path)?;

                apply_meta_to_symlink(target, name, &inode, &entry_path)?;
            } else if let Some(content_checksum) = inode.content() {
                let mut file = target
                    .create(name)
                    .io_context(Operation::CreateFile, &entry_path)?
                    .into_std();
                let file_content = store.load(content_checksum)?;
                file_content
                    .write_to(&mut file)
                    .io_context(Operation::WriteFile, &entry_path)?;

                let file = target
                    .open(name)
                    .io_context(Operation::OpenFile, &entry_path)?;
                apply_meta_to_file(&file, &inode, &entry_path)?;
            }
        }
    }

    // Pass 2: Create all hardlinks (targets guaranteed to exist)
    for (name, entry) in dir.entries() {
        if let DirEntryObject::Hardlink { first_target, .. } = entry {
            let entry_path = current_path.join(name);
            root_target
                .hard_link(first_target, target, name)
                .io_context(Operation::CreateHardlink, &entry_path)?;
        }
    }

    // Pass 3: Create and recurse into subdirectories
    for (name, entry) in dir.entries() {
        if let DirEntryObject::Dir(subdir_checksum) = entry {
            let entry_path = current_path.join(name);
            target
                .create_dir(name)
                .io_context(Operation::CreateDir, &entry_path)?;
            let subdir = target
                .open_dir(name)
                .io_context(Operation::OpenDir, &entry_path)?;
            extract_dir_recursive_impl(store, subdir_checksum, &subdir, root_target, &entry_path)?;
        }
    }

    Ok(())
}

fn get_dir_meta(dir: &fs::Dir, path: &Path) -> Result<Inode> {
    let stat = dir
        .dir_metadata()
        .io_context(Operation::ReadMetadata, path)?;
    let xattrs = get_xattrs_from_dir(dir).io_context(Operation::ReadXattrs, path)?;
    Ok(Inode::builder()
        .uid(stat.uid())
        .gid(stat.gid())
        .mode(stat.mode())
        .xattrs(xattrs)
        .data(Data::Directory)
        .build())
}

fn get_file_meta(store: &ObjectStore, f: fs::File, path: &Path) -> Result<Inode> {
    let stat = f.metadata().io_context(Operation::ReadMetadata, path)?;
    let (f, xattrs) = get_xattrs_from_fd(f).io_context(Operation::ReadXattrs, path)?;
    let content = FileContent::from_cap_std_file(f.into_std());
    let content_checksum = store.store(&content)?;
    Ok(Inode::builder()
        .uid(stat.uid())
        .gid(stat.gid())
        .mode(stat.mode())
        .xattrs(xattrs)
        .data(Data::RegularFile(content_checksum))
        .build())
}

fn get_symlink_meta(dir: &fs::Dir, ent: &DirEntry, path: &Path) -> Result<Inode> {
    let target =
        readlinkat(dir.as_fd(), ent.file_name(), vec![]).io_context(Operation::ReadLink, path)?;
    let stat = dir
        .symlink_metadata(ent.file_name())
        .io_context(Operation::ReadMetadata, path)?;

    use std::os::unix::ffi::OsStrExt;
    let target_path = std::path::PathBuf::from(std::ffi::OsStr::from_bytes(target.as_bytes()));

    Ok(Inode::builder()
        .uid(stat.uid())
        .gid(stat.gid())
        .mode(stat.mode())
        // Symlinks don't support xattrs on Linux
        .data(Data::Symlink(target_path))
        .build())
}

/// Read all extended attributes from a file descriptor.
fn get_xattrs_from_fd(
    file: cap_std::fs::File,
) -> std::io::Result<(cap_std::fs::File, BTreeMap<OsString, Vec<u8>>)> {
    let std = file.into_std();
    get_xattrs_from_std_file(std).map(|(f, xattrs)| (cap_std::fs::File::from_std(f), xattrs))
}

fn get_xattrs_from_std_file(
    file: std::fs::File,
) -> std::io::Result<(std::fs::File, BTreeMap<OsString, Vec<u8>>)> {
    let mut xattrs = BTreeMap::new();
    match file.list_xattr() {
        Ok(names) => {
            for name in names {
                if let Ok(Some(value)) = file.get_xattr(&name) {
                    xattrs.insert(name, value);
                }
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // File doesn't exist or no xattr support, return empty map
        }
        Err(e) if e.kind() == std::io::ErrorKind::Unsupported => {
            // Filesystem doesn't support xattrs, return empty map
        }
        Err(e) => return Err(e),
    }
    Ok((file, xattrs))
}

/// Read extended attributes from a directory.
///
/// cap_std::fs::Dir may be opened with O_PATH, which doesn't support xattr
/// operations. Re-open "." with O_RDONLY via openat to get a usable fd.
fn get_xattrs_from_dir(dir: &fs::Dir) -> std::io::Result<BTreeMap<OsString, Vec<u8>>> {
    let fd = rustix::fs::openat(
        dir,
        ".",
        rustix::fs::OFlags::RDONLY | rustix::fs::OFlags::DIRECTORY,
        rustix::fs::Mode::empty(),
    )
    .map_err(std::io::Error::from)?;
    let file = std::fs::File::from(fd);
    let (_, xattrs) = get_xattrs_from_std_file(file)?;
    Ok(xattrs)
}

fn apply_meta_to_dir(dir: &fs::Dir, meta: &Inode, path: &Path) -> Result<()> {
    // Set ownership first.  chown clears setuid/setgid bits, so it must come
    // before chmod.
    // Use chownat with "." instead of fchown because cap_std::fs::Dir may be
    // opened with O_PATH, which doesn't support fchown.
    let uid = Uid::from_raw(meta.uid());
    let gid = Gid::from_raw(meta.gid());
    chownat(dir, ".", Some(uid), Some(gid), AtFlags::empty())
        .io_context(Operation::SetOwnership, path)?;

    // Set permissions after chown so setuid/setgid/sticky bits survive
    let permissions = fs::Permissions::from_mode(meta.mode() & 0o7777);
    dir.set_permissions(".", permissions)
        .io_context(Operation::SetPermissions, path)?;

    // Set xattrs - re-open with O_RDONLY since the Dir's O_PATH fd doesn't
    // support xattr operations.
    if !meta.xattrs().is_empty() {
        let fd = rustix::fs::openat(
            dir,
            ".",
            rustix::fs::OFlags::RDONLY | rustix::fs::OFlags::DIRECTORY,
            rustix::fs::Mode::empty(),
        )
        .io_context(Operation::SetXattrs, path)?;
        let file = std::fs::File::from(fd);
        for (name, value) in meta.xattrs() {
            file.set_xattr(name, value)
                .io_context(Operation::SetXattrs, path)?;
        }
    }

    Ok(())
}

fn apply_meta_to_file(file: &fs::File, meta: &Inode, path: &Path) -> Result<()> {
    // Set ownership first.
    // chown clears setuid/setgid bits, so it must come before chmod.
    let uid = Uid::from_raw(meta.uid());
    let gid = Gid::from_raw(meta.gid());
    fchown(file, Some(uid), Some(gid)).io_context(Operation::SetOwnership, path)?;

    // Set permissions after chown so setuid/setgid/sticky bits survive
    let permissions = fs::Permissions::from_mode(meta.mode() & 0o7777);
    file.set_permissions(permissions)
        .io_context(Operation::SetPermissions, path)?;

    // Set xattrs
    set_xattrs_on_fd(file.as_fd(), meta.xattrs(), path)?;

    Ok(())
}

fn apply_meta_to_symlink(dir: &fs::Dir, name: &str, meta: &Inode, path: &Path) -> Result<()> {
    // Symlinks don't have permissions in the traditional sense on Linux,
    // but we can set ownership using chownat with AT_SYMLINK_NOFOLLOW
    let uid = Uid::from_raw(meta.uid());
    let gid = Gid::from_raw(meta.gid());
    chownat(dir, name, Some(uid), Some(gid), AtFlags::SYMLINK_NOFOLLOW)
        .io_context(Operation::SetOwnership, path)?;

    // Symlinks typically don't have xattrs on Linux and only a subset is
    // supported, but we should try to set that subset
    if !meta.xattrs().is_empty() {
        let fd = dir
            .open_with(
                name,
                OpenOptions::new()
                    .read(true)
                    .custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32),
            )
            .io_context(Operation::OpenFile, path)?;
        set_xattrs_on_fd(fd, meta.xattrs(), path)?;
    }

    Ok(())
}

/// Set extended attributes on a file descriptor.
fn set_xattrs_on_fd<Fd: std::os::fd::AsFd>(
    fd: Fd,
    xattrs: &BTreeMap<OsString, Vec<u8>>,
    path: &Path,
) -> Result<()> {
    let file = std::fs::File::from(
        fd.as_fd()
            .try_clone_to_owned()
            .io_context(Operation::SetXattrs, path)?,
    );
    for (name, value) in xattrs {
        file.set_xattr(name, value)
            .io_context(Operation::SetXattrs, path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::MetadataExt;
    use std::os::unix::fs::PermissionsExt;

    use tempfile::TempDir;

    use super::*;

    fn create_test_store() -> (TempDir, ObjectStore) {
        let store_tmpdir = TempDir::new().expect("failed to create tmpdir");
        let store =
            ObjectStore::new_from_empty(store_tmpdir.path()).expect("failed to create ObjectStore");
        (store_tmpdir, store)
    }

    #[test]
    fn test_add_empty_directory() {
        let (_store_tmpdir, store) = create_test_store();

        let tmpdir = TempDir::new().expect("failed to create tmpdir");
        let root_dir = fs::Dir::open_ambient_dir(tmpdir.path(), cap_std::ambient_authority())
            .expect("failed to open root dir");
        let root = add_dir_recursive(&store, root_dir).expect("failed to add root dir");

        let root_obj = store.load(&root).expect("failed to load root dir object");
        assert!(
            root_obj.is_empty(),
            "empty directory should have no entries"
        );
    }

    #[test]
    fn test_add_nested_directories() {
        let (_store_tmpdir, store) = create_test_store();

        let tmpdir = TempDir::new().expect("failed to create tmpdir");
        std::fs::create_dir_all(tmpdir.path().join("a/b/c")).expect("failed to create dir a/b/c");
        std::fs::create_dir(tmpdir.path().join("x")).expect("failed to create dir x");

        let root_dir = fs::Dir::open_ambient_dir(tmpdir.path(), cap_std::ambient_authority())
            .expect("failed to open root dir");
        let root = add_dir_recursive(&store, root_dir).expect("failed to add root dir");

        let root_obj = store.load(&root).expect("failed to load root dir object");
        assert_eq!(root_obj.len(), 2, "root should have 2 entries: a and x");

        let DirEntryObject::Dir(a_checksum) = root_obj.get("a").expect("should have entry 'a'")
        else {
            panic!("'a' should be a directory");
        };

        assert!(
            matches!(root_obj.get("x"), Some(DirEntryObject::Dir(_))),
            "'x' should be a directory"
        );

        let a_obj = store.load(a_checksum).expect("failed to load 'a' dir");
        assert_eq!(a_obj.len(), 1, "'a' should have 1 entry: b");

        let DirEntryObject::Dir(b_checksum) = a_obj.get("b").expect("should have entry 'b'") else {
            panic!("'b' should be a directory");
        };
        let b_obj = store.load(b_checksum).expect("failed to load 'b' dir");
        assert_eq!(b_obj.len(), 1, "'b' should have 1 entry: c");

        let DirEntryObject::Dir(c_checksum) = b_obj.get("c").expect("should have entry 'c'") else {
            panic!("'c' should be a directory");
        };
        let c_obj = store.load(c_checksum).expect("failed to load 'c' dir");
        assert!(c_obj.is_empty(), "'c' should be empty");
    }

    #[test]
    fn test_add_regular_files() {
        let (_store_tmpdir, store) = create_test_store();

        let tmpdir = TempDir::new().expect("failed to create tmpdir");
        std::fs::write(tmpdir.path().join("hello.txt"), "Hello, world!")
            .expect("failed to write hello.txt");
        std::fs::write(tmpdir.path().join("empty.txt"), "").expect("failed to write empty.txt");
        std::fs::write(tmpdir.path().join("binary"), vec![0u8, 1, 2, 255, 254, 253])
            .expect("failed to write binary");

        let root_dir = fs::Dir::open_ambient_dir(tmpdir.path(), cap_std::ambient_authority())
            .expect("failed to open root dir");
        let root = add_dir_recursive(&store, root_dir).expect("failed to add root dir");

        let root_obj = store.load(&root).expect("failed to load root dir object");
        assert_eq!(root_obj.len(), 3, "root should have 3 entries");

        assert!(
            matches!(root_obj.get("hello.txt"), Some(DirEntryObject::File(_))),
            "hello.txt should be a file"
        );

        assert!(
            matches!(root_obj.get("empty.txt"), Some(DirEntryObject::File(_))),
            "empty.txt should be a file"
        );

        assert!(
            matches!(root_obj.get("binary"), Some(DirEntryObject::File(_))),
            "binary should be a file"
        );
    }

    #[test]
    fn test_add_symlinks() {
        let (_store_tmpdir, store) = create_test_store();

        let tmpdir = TempDir::new().expect("failed to create tmpdir");
        std::fs::write(tmpdir.path().join("target.txt"), "I am the target")
            .expect("failed to write target.txt");

        std::os::unix::fs::symlink("target.txt", tmpdir.path().join("relative_link"))
            .expect("failed to create relative symlink");
        std::os::unix::fs::symlink("/absolute/path", tmpdir.path().join("absolute_link"))
            .expect("failed to create absolute symlink");
        std::os::unix::fs::symlink("nonexistent", tmpdir.path().join("broken_link"))
            .expect("failed to create broken symlink");

        let root_dir = fs::Dir::open_ambient_dir(tmpdir.path(), cap_std::ambient_authority())
            .expect("failed to open root dir");
        let root = add_dir_recursive(&store, root_dir).expect("failed to add root dir");

        let root_obj = store.load(&root).expect("failed to load root dir object");
        assert_eq!(root_obj.len(), 4, "root should have 4 entries");

        assert!(
            matches!(root_obj.get("relative_link"), Some(DirEntryObject::File(_))),
            "relative_link should be a file (symlink)"
        );

        assert!(
            matches!(root_obj.get("absolute_link"), Some(DirEntryObject::File(_))),
            "absolute_link should be a file (symlink)"
        );

        assert!(
            matches!(root_obj.get("broken_link"), Some(DirEntryObject::File(_))),
            "broken_link should be a file (symlink)"
        );
    }

    #[test]
    fn test_add_mixed_content() {
        let (_store_tmpdir, store) = create_test_store();

        let tmpdir = TempDir::new().expect("failed to create tmpdir");

        std::fs::create_dir_all(tmpdir.path().join("subdir")).expect("failed to create subdir");
        std::fs::write(tmpdir.path().join("file.txt"), "root file")
            .expect("failed to write file.txt");
        std::fs::write(tmpdir.path().join("subdir/nested.txt"), "nested file")
            .expect("failed to write nested.txt");
        std::os::unix::fs::symlink("file.txt", tmpdir.path().join("link"))
            .expect("failed to create symlink");
        std::os::unix::fs::symlink("../file.txt", tmpdir.path().join("subdir/uplink"))
            .expect("failed to create uplink");

        let root_dir = fs::Dir::open_ambient_dir(tmpdir.path(), cap_std::ambient_authority())
            .expect("failed to open root dir");
        let root = add_dir_recursive(&store, root_dir).expect("failed to add root dir");

        let root_obj = store.load(&root).expect("failed to load root dir object");
        assert_eq!(root_obj.len(), 3, "root should have 3 entries");

        assert!(
            matches!(root_obj.get("file.txt"), Some(DirEntryObject::File(_))),
            "file.txt should be a file"
        );
        assert!(
            matches!(root_obj.get("link"), Some(DirEntryObject::File(_))),
            "link should be a file (symlink)"
        );

        let DirEntryObject::Dir(subdir_checksum) =
            root_obj.get("subdir").expect("should have subdir")
        else {
            panic!("subdir should be a directory");
        };
        let subdir_obj = store.load(subdir_checksum).expect("failed to load subdir");
        assert_eq!(subdir_obj.len(), 2, "subdir should have 2 entries");
        assert!(
            matches!(subdir_obj.get("nested.txt"), Some(DirEntryObject::File(_))),
            "nested.txt should be a file"
        );
        assert!(
            matches!(subdir_obj.get("uplink"), Some(DirEntryObject::File(_))),
            "uplink should be a file (symlink)"
        );
    }

    #[test]
    fn test_extract_empty_directory() {
        let (_store_tmpdir, store) = create_test_store();

        let source_tmpdir = TempDir::new().expect("failed to create source tmpdir");
        let source_dir =
            fs::Dir::open_ambient_dir(source_tmpdir.path(), cap_std::ambient_authority())
                .expect("failed to open source dir");
        let root_checksum =
            add_dir_recursive(&store, source_dir).expect("failed to add source dir");

        let target_tmpdir = TempDir::new().expect("failed to create target tmpdir");
        let target_dir =
            fs::Dir::open_ambient_dir(target_tmpdir.path(), cap_std::ambient_authority())
                .expect("failed to open target dir");
        extract_dir_recursive(&store, &root_checksum, &target_dir).expect("failed to extract");

        let entries: Vec<_> = std::fs::read_dir(target_tmpdir.path())
            .expect("failed to read target dir")
            .collect();
        assert!(entries.is_empty(), "target should be empty");
    }

    #[test]
    fn test_extract_regular_files() {
        let (_store_tmpdir, store) = create_test_store();

        let source_tmpdir = TempDir::new().expect("failed to create source tmpdir");
        std::fs::write(source_tmpdir.path().join("hello.txt"), "Hello, world!")
            .expect("failed to write hello.txt");
        std::fs::write(source_tmpdir.path().join("binary"), vec![0u8, 1, 2, 255])
            .expect("failed to write binary");

        let source_dir =
            fs::Dir::open_ambient_dir(source_tmpdir.path(), cap_std::ambient_authority())
                .expect("failed to open source dir");
        let root_checksum =
            add_dir_recursive(&store, source_dir).expect("failed to add source dir");

        let target_tmpdir = TempDir::new().expect("failed to create target tmpdir");
        let target_dir =
            fs::Dir::open_ambient_dir(target_tmpdir.path(), cap_std::ambient_authority())
                .expect("failed to open target dir");
        extract_dir_recursive(&store, &root_checksum, &target_dir).expect("failed to extract");

        let hello_content = std::fs::read_to_string(target_tmpdir.path().join("hello.txt"))
            .expect("failed to read hello.txt");
        assert_eq!(hello_content, "Hello, world!");

        let binary_content =
            std::fs::read(target_tmpdir.path().join("binary")).expect("failed to read binary");
        assert_eq!(binary_content, vec![0u8, 1, 2, 255]);
    }

    #[test]
    fn test_extract_symlinks() {
        let (_store_tmpdir, store) = create_test_store();

        let source_tmpdir = TempDir::new().expect("failed to create source tmpdir");
        std::fs::write(source_tmpdir.path().join("target.txt"), "target content")
            .expect("failed to write target.txt");
        std::os::unix::fs::symlink("target.txt", source_tmpdir.path().join("link"))
            .expect("failed to create symlink");
        std::os::unix::fs::symlink("/absolute/path", source_tmpdir.path().join("abslink"))
            .expect("failed to create absolute symlink");

        let source_dir =
            fs::Dir::open_ambient_dir(source_tmpdir.path(), cap_std::ambient_authority())
                .expect("failed to open source dir");
        let root_checksum =
            add_dir_recursive(&store, source_dir).expect("failed to add source dir");

        let target_tmpdir = TempDir::new().expect("failed to create target tmpdir");
        let target_dir =
            fs::Dir::open_ambient_dir(target_tmpdir.path(), cap_std::ambient_authority())
                .expect("failed to open target dir");
        extract_dir_recursive(&store, &root_checksum, &target_dir).expect("failed to extract");

        let link_target = std::fs::read_link(target_tmpdir.path().join("link"))
            .expect("failed to read link target");
        assert_eq!(link_target.to_string_lossy(), "target.txt");

        let abslink_target = std::fs::read_link(target_tmpdir.path().join("abslink"))
            .expect("failed to read abslink target");
        assert_eq!(abslink_target.to_string_lossy(), "/absolute/path");

        let content = std::fs::read_to_string(target_tmpdir.path().join("link"))
            .expect("failed to read through symlink");
        assert_eq!(content, "target content");
    }

    #[test]
    fn test_extract_nested_directories() {
        let (_store_tmpdir, store) = create_test_store();

        let source_tmpdir = TempDir::new().expect("failed to create source tmpdir");
        std::fs::create_dir_all(source_tmpdir.path().join("a/b/c"))
            .expect("failed to create a/b/c");
        std::fs::write(source_tmpdir.path().join("a/file1.txt"), "file1")
            .expect("failed to write file1.txt");
        std::fs::write(source_tmpdir.path().join("a/b/file2.txt"), "file2")
            .expect("failed to write file2.txt");
        std::fs::write(source_tmpdir.path().join("a/b/c/file3.txt"), "file3")
            .expect("failed to write file3.txt");

        let source_dir =
            fs::Dir::open_ambient_dir(source_tmpdir.path(), cap_std::ambient_authority())
                .expect("failed to open source dir");
        let root_checksum =
            add_dir_recursive(&store, source_dir).expect("failed to add source dir");

        let target_tmpdir = TempDir::new().expect("failed to create target tmpdir");
        let target_dir =
            fs::Dir::open_ambient_dir(target_tmpdir.path(), cap_std::ambient_authority())
                .expect("failed to open target dir");
        extract_dir_recursive(&store, &root_checksum, &target_dir).expect("failed to extract");

        assert!(target_tmpdir.path().join("a").is_dir());
        assert!(target_tmpdir.path().join("a/b").is_dir());
        assert!(target_tmpdir.path().join("a/b/c").is_dir());

        assert_eq!(
            std::fs::read_to_string(target_tmpdir.path().join("a/file1.txt"))
                .expect("failed to read file1"),
            "file1"
        );
        assert_eq!(
            std::fs::read_to_string(target_tmpdir.path().join("a/b/file2.txt"))
                .expect("failed to read file2"),
            "file2"
        );
        assert_eq!(
            std::fs::read_to_string(target_tmpdir.path().join("a/b/c/file3.txt"))
                .expect("failed to read file3"),
            "file3"
        );
    }

    #[test]
    fn test_extract_preserves_file_permissions() {
        let (_store_tmpdir, store) = create_test_store();

        let source_tmpdir = TempDir::new().expect("failed to create source tmpdir");
        let file_path = source_tmpdir.path().join("executable");
        std::fs::write(&file_path, "#!/bin/sh\necho hello").expect("failed to write executable");

        let mut perms = std::fs::metadata(&file_path)
            .expect("failed to get metadata")
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&file_path, perms).expect("failed to set permissions");

        let source_dir =
            fs::Dir::open_ambient_dir(source_tmpdir.path(), cap_std::ambient_authority())
                .expect("failed to open source dir");
        let root_checksum =
            add_dir_recursive(&store, source_dir).expect("failed to add source dir");

        let target_tmpdir = TempDir::new().expect("failed to create target tmpdir");
        let target_dir =
            fs::Dir::open_ambient_dir(target_tmpdir.path(), cap_std::ambient_authority())
                .expect("failed to open target dir");
        extract_dir_recursive(&store, &root_checksum, &target_dir).expect("failed to extract");

        let target_meta =
            std::fs::metadata(target_tmpdir.path().join("executable")).expect("failed to get meta");
        let mode = target_meta.permissions().mode() & 0o7777;
        assert_eq!(mode, 0o755, "executable permissions should be preserved");
    }

    #[test]
    fn test_round_trip_complex_tree() {
        let (_store_tmpdir, store) = create_test_store();

        let source_tmpdir = TempDir::new().expect("failed to create source tmpdir");
        let base = source_tmpdir.path();

        std::fs::create_dir_all(base.join("src/lib")).expect("failed to create src/lib");
        std::fs::create_dir_all(base.join("tests")).expect("failed to create tests");
        std::fs::create_dir(base.join("bin")).expect("failed to create bin");

        std::fs::write(base.join("README.md"), "# My Project\n").expect("failed to write README");
        std::fs::write(base.join("src/main.rs"), "fn main() {}").expect("failed to write main.rs");
        std::fs::write(base.join("src/lib/mod.rs"), "pub mod foo;")
            .expect("failed to write mod.rs");
        std::fs::write(base.join("src/lib/foo.rs"), "pub fn foo() {}")
            .expect("failed to write foo.rs");
        std::fs::write(base.join("tests/test.rs"), "#[test] fn test() {}")
            .expect("failed to write test.rs");

        std::os::unix::fs::symlink("../src/main.rs", base.join("bin/main"))
            .expect("failed to create symlink");
        std::os::unix::fs::symlink("lib", base.join("src/library"))
            .expect("failed to create symlink");

        let mut perms = std::fs::metadata(base.join("src/main.rs"))
            .expect("failed to get metadata")
            .permissions();
        perms.set_mode(0o644);
        std::fs::set_permissions(base.join("src/main.rs"), perms)
            .expect("failed to set permissions");

        let source_dir = fs::Dir::open_ambient_dir(base, cap_std::ambient_authority())
            .expect("failed to open source dir");
        let root_checksum =
            add_dir_recursive(&store, source_dir).expect("failed to add source dir");

        let target_tmpdir = TempDir::new().expect("failed to create target tmpdir");
        let target_dir =
            fs::Dir::open_ambient_dir(target_tmpdir.path(), cap_std::ambient_authority())
                .expect("failed to open target dir");
        extract_dir_recursive(&store, &root_checksum, &target_dir).expect("failed to extract");

        let target = target_tmpdir.path();

        assert!(target.join("src").is_dir());
        assert!(target.join("src/lib").is_dir());
        assert!(target.join("tests").is_dir());
        assert!(target.join("bin").is_dir());

        assert_eq!(
            std::fs::read_to_string(target.join("README.md")).expect("failed to read README"),
            "# My Project\n"
        );
        assert_eq!(
            std::fs::read_to_string(target.join("src/main.rs")).expect("failed to read main.rs"),
            "fn main() {}"
        );
        assert_eq!(
            std::fs::read_to_string(target.join("src/lib/mod.rs")).expect("failed to read mod.rs"),
            "pub mod foo;"
        );
        assert_eq!(
            std::fs::read_to_string(target.join("src/lib/foo.rs")).expect("failed to read foo.rs"),
            "pub fn foo() {}"
        );
        assert_eq!(
            std::fs::read_to_string(target.join("tests/test.rs")).expect("failed to read test.rs"),
            "#[test] fn test() {}"
        );

        let bin_main_target =
            std::fs::read_link(target.join("bin/main")).expect("failed to read link");
        assert_eq!(bin_main_target.to_string_lossy(), "../src/main.rs");

        let library_target =
            std::fs::read_link(target.join("src/library")).expect("failed to read link");
        assert_eq!(library_target.to_string_lossy(), "lib");

        let main_mode = std::fs::metadata(target.join("src/main.rs"))
            .expect("failed to get metadata")
            .permissions()
            .mode()
            & 0o7777;
        assert_eq!(main_mode, 0o644);
    }

    #[test]
    fn test_identical_content_deduplicated() {
        let (_store_tmpdir, store) = create_test_store();

        let source_tmpdir = TempDir::new().expect("failed to create source tmpdir");
        let content = "This is the same content\n".repeat(100);

        std::fs::create_dir(source_tmpdir.path().join("dir1")).expect("failed to create dir1");
        std::fs::create_dir(source_tmpdir.path().join("dir2")).expect("failed to create dir2");

        std::fs::write(source_tmpdir.path().join("file1.txt"), &content)
            .expect("failed to write file1");
        std::fs::write(source_tmpdir.path().join("dir1/file2.txt"), &content)
            .expect("failed to write file2");
        std::fs::write(source_tmpdir.path().join("dir2/file3.txt"), &content)
            .expect("failed to write file3");

        let source_dir =
            fs::Dir::open_ambient_dir(source_tmpdir.path(), cap_std::ambient_authority())
                .expect("failed to open source dir");
        let root_checksum =
            add_dir_recursive(&store, source_dir).expect("failed to add source dir");

        let root = store.load(&root_checksum).expect("failed to load root");

        // Get content checksums from all three files and compare by equality
        let DirEntryObject::File(file1_inode_checksum) =
            root.get("file1.txt").expect("should have file1")
        else {
            panic!("file1.txt should be a file");
        };
        let file1_inode = store
            .load(file1_inode_checksum)
            .expect("failed to load file1 inode");
        let file1_content = file1_inode.content().expect("file1 should have content");

        let DirEntryObject::Dir(dir1_checksum) = root.get("dir1").expect("should have dir1") else {
            panic!("dir1 should be a directory");
        };
        let dir1 = store.load(dir1_checksum).expect("failed to load dir1");
        let DirEntryObject::File(file2_inode_checksum) =
            dir1.get("file2.txt").expect("should have file2")
        else {
            panic!("file2.txt should be a file");
        };
        let file2_inode = store
            .load(file2_inode_checksum)
            .expect("failed to load file2 inode");
        let file2_content = file2_inode.content().expect("file2 should have content");

        let DirEntryObject::Dir(dir2_checksum) = root.get("dir2").expect("should have dir2") else {
            panic!("dir2 should be a directory");
        };
        let dir2 = store.load(dir2_checksum).expect("failed to load dir2");
        let DirEntryObject::File(file3_inode_checksum) =
            dir2.get("file3.txt").expect("should have file3")
        else {
            panic!("file3.txt should be a file");
        };
        let file3_inode = store
            .load(file3_inode_checksum)
            .expect("failed to load file3 inode");
        let file3_content = file3_inode.content().expect("file3 should have content");

        // All three should reference the same content object
        assert_eq!(file1_content, file2_content);
        assert_eq!(file2_content, file3_content);
    }

    #[test]
    fn test_dir_entry_helpers() {
        let (_store_tmpdir, store) = create_test_store();

        let source_tmpdir = TempDir::new().expect("failed to create source tmpdir");
        std::fs::create_dir(source_tmpdir.path().join("subdir")).expect("failed to create subdir");
        std::fs::write(source_tmpdir.path().join("file.txt"), "content")
            .expect("failed to write file");
        std::os::unix::fs::symlink("file.txt", source_tmpdir.path().join("link"))
            .expect("failed to create symlink");

        let source_dir =
            fs::Dir::open_ambient_dir(source_tmpdir.path(), cap_std::ambient_authority())
                .expect("failed to open source dir");
        let root_checksum =
            add_dir_recursive(&store, source_dir).expect("failed to add source dir");

        let root = store.load(&root_checksum).expect("failed to load root");

        assert!(
            matches!(root.get("subdir"), Some(DirEntryObject::Dir(_))),
            "subdir should be a directory"
        );

        let DirEntryObject::File(file_inode) = root.get("file.txt").expect("should have file.txt")
        else {
            panic!("file.txt should be a file");
        };
        let file_inode = store.load(file_inode).expect("failed to load file inode");
        assert!(
            file_inode.content().is_some(),
            "file.txt should be a regular file"
        );

        let DirEntryObject::File(link_inode) = root.get("link").expect("should have link") else {
            panic!("link should be a file");
        };
        let link_inode = store.load(link_inode).expect("failed to load link inode");
        assert_eq!(
            link_inode
                .link_target()
                .map(|p| p.to_string_lossy().to_string()),
            Some("file.txt".to_string()),
            "link should be a symlink to file.txt"
        );
    }

    #[test]
    fn test_entries_sorted_order() {
        let (_store_tmpdir, store) = create_test_store();

        let source_tmpdir = TempDir::new().expect("failed to create source tmpdir");

        std::fs::write(source_tmpdir.path().join("zebra"), "z").expect("failed to write zebra");
        std::fs::write(source_tmpdir.path().join("apple"), "a").expect("failed to write apple");
        std::fs::write(source_tmpdir.path().join("mango"), "m").expect("failed to write mango");

        let source_dir =
            fs::Dir::open_ambient_dir(source_tmpdir.path(), cap_std::ambient_authority())
                .expect("failed to open source dir");
        let root_checksum =
            add_dir_recursive(&store, source_dir).expect("failed to add source dir");

        let root = store.load(&root_checksum).expect("failed to load root");

        let names: Vec<_> = root.entries().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names, vec!["apple", "mango", "zebra"]);
    }

    #[test]
    fn test_large_file() {
        let (_store_tmpdir, store) = create_test_store();

        let source_tmpdir = TempDir::new().expect("failed to create source tmpdir");

        let large_content: Vec<u8> = (0..1024 * 1024).map(|i| (i % 256) as u8).collect();
        std::fs::write(source_tmpdir.path().join("large.bin"), &large_content)
            .expect("failed to write large file");

        let source_dir =
            fs::Dir::open_ambient_dir(source_tmpdir.path(), cap_std::ambient_authority())
                .expect("failed to open source dir");
        let root_checksum =
            add_dir_recursive(&store, source_dir).expect("failed to add source dir");

        let target_tmpdir = TempDir::new().expect("failed to create target tmpdir");
        let target_dir =
            fs::Dir::open_ambient_dir(target_tmpdir.path(), cap_std::ambient_authority())
                .expect("failed to open target dir");
        extract_dir_recursive(&store, &root_checksum, &target_dir).expect("failed to extract");

        let extracted = std::fs::read(target_tmpdir.path().join("large.bin"))
            .expect("failed to read large file");
        assert_eq!(extracted, large_content);
    }

    #[test]
    fn test_special_characters_in_names() {
        let (_store_tmpdir, store) = create_test_store();

        let source_tmpdir = TempDir::new().expect("failed to create source tmpdir");

        std::fs::write(source_tmpdir.path().join("file with spaces.txt"), "spaces")
            .expect("failed to write file");
        std::fs::write(source_tmpdir.path().join("file-with-dashes"), "dashes")
            .expect("failed to write file");
        std::fs::write(
            source_tmpdir.path().join("file_with_underscores"),
            "underscores",
        )
        .expect("failed to write file");
        std::fs::write(source_tmpdir.path().join("file.multiple.dots.txt"), "dots")
            .expect("failed to write file");

        let source_dir =
            fs::Dir::open_ambient_dir(source_tmpdir.path(), cap_std::ambient_authority())
                .expect("failed to open source dir");
        let root_checksum =
            add_dir_recursive(&store, source_dir).expect("failed to add source dir");

        let target_tmpdir = TempDir::new().expect("failed to create target tmpdir");
        let target_dir =
            fs::Dir::open_ambient_dir(target_tmpdir.path(), cap_std::ambient_authority())
                .expect("failed to open target dir");
        extract_dir_recursive(&store, &root_checksum, &target_dir).expect("failed to extract");

        assert_eq!(
            std::fs::read_to_string(target_tmpdir.path().join("file with spaces.txt"))
                .expect("failed to read file"),
            "spaces"
        );
        assert_eq!(
            std::fs::read_to_string(target_tmpdir.path().join("file-with-dashes"))
                .expect("failed to read file"),
            "dashes"
        );
        assert_eq!(
            std::fs::read_to_string(target_tmpdir.path().join("file_with_underscores"))
                .expect("failed to read file"),
            "underscores"
        );
        assert_eq!(
            std::fs::read_to_string(target_tmpdir.path().join("file.multiple.dots.txt"))
                .expect("failed to read file"),
            "dots"
        );
    }

    #[test]
    fn test_empty_file() {
        let (_store_tmpdir, store) = create_test_store();

        let source_tmpdir = TempDir::new().expect("failed to create source tmpdir");
        std::fs::write(source_tmpdir.path().join("empty"), "").expect("failed to write empty file");

        let source_dir =
            fs::Dir::open_ambient_dir(source_tmpdir.path(), cap_std::ambient_authority())
                .expect("failed to open source dir");
        let root_checksum =
            add_dir_recursive(&store, source_dir).expect("failed to add source dir");

        let target_tmpdir = TempDir::new().expect("failed to create target tmpdir");
        let target_dir =
            fs::Dir::open_ambient_dir(target_tmpdir.path(), cap_std::ambient_authority())
                .expect("failed to open target dir");
        extract_dir_recursive(&store, &root_checksum, &target_dir).expect("failed to extract");

        let content =
            std::fs::read(target_tmpdir.path().join("empty")).expect("failed to read empty file");
        assert!(content.is_empty());
    }

    #[test]
    fn test_symlink_to_directory() {
        let (_store_tmpdir, store) = create_test_store();

        let source_tmpdir = TempDir::new().expect("failed to create source tmpdir");
        std::fs::create_dir(source_tmpdir.path().join("actual_dir")).expect("failed to create dir");
        std::fs::write(source_tmpdir.path().join("actual_dir/file.txt"), "in dir")
            .expect("failed to write file");
        std::os::unix::fs::symlink("actual_dir", source_tmpdir.path().join("dir_link"))
            .expect("failed to create symlink");

        let source_dir =
            fs::Dir::open_ambient_dir(source_tmpdir.path(), cap_std::ambient_authority())
                .expect("failed to open source dir");
        let root_checksum =
            add_dir_recursive(&store, source_dir).expect("failed to add source dir");

        let root = store.load(&root_checksum).expect("failed to load root");
        assert!(
            matches!(root.get("actual_dir"), Some(DirEntryObject::Dir(_))),
            "actual_dir should be a directory"
        );

        let DirEntryObject::File(dir_link_inode) =
            root.get("dir_link").expect("should have dir_link")
        else {
            panic!("dir_link should be a file");
        };
        let dir_link_inode = store
            .load(dir_link_inode)
            .expect("failed to load dir_link inode");
        assert_eq!(
            dir_link_inode
                .link_target()
                .map(|p| p.to_string_lossy().to_string()),
            Some("actual_dir".to_string()),
            "dir_link should be a symlink to actual_dir"
        );

        let target_tmpdir = TempDir::new().expect("failed to create target tmpdir");
        let target_dir =
            fs::Dir::open_ambient_dir(target_tmpdir.path(), cap_std::ambient_authority())
                .expect("failed to open target dir");
        extract_dir_recursive(&store, &root_checksum, &target_dir).expect("failed to extract");

        let link_target =
            std::fs::read_link(target_tmpdir.path().join("dir_link")).expect("failed to read link");
        assert_eq!(link_target.to_string_lossy(), "actual_dir");

        // Following the symlink should work
        let content = std::fs::read_to_string(target_tmpdir.path().join("dir_link/file.txt"))
            .expect("failed to read file");
        assert_eq!(content, "in dir");
    }

    #[test]
    fn test_multiple_extracts_same_source() {
        let (_store_tmpdir, store) = create_test_store();

        let source_tmpdir = TempDir::new().expect("failed to create source tmpdir");
        std::fs::write(source_tmpdir.path().join("file.txt"), "content")
            .expect("failed to write file");

        let source_dir =
            fs::Dir::open_ambient_dir(source_tmpdir.path(), cap_std::ambient_authority())
                .expect("failed to open source dir");
        let root_checksum =
            add_dir_recursive(&store, source_dir).expect("failed to add source dir");

        // Extract to first target
        let target1 = TempDir::new().expect("failed to create target1");
        let target1_dir = fs::Dir::open_ambient_dir(target1.path(), cap_std::ambient_authority())
            .expect("failed to open target1");
        extract_dir_recursive(&store, &root_checksum, &target1_dir)
            .expect("failed to extract to target1");

        // Extract to second target
        let target2 = TempDir::new().expect("failed to create target2");
        let target2_dir = fs::Dir::open_ambient_dir(target2.path(), cap_std::ambient_authority())
            .expect("failed to open target2");
        extract_dir_recursive(&store, &root_checksum, &target2_dir)
            .expect("failed to extract to target2");

        // Both should have the same content
        assert_eq!(
            std::fs::read_to_string(target1.path().join("file.txt")).expect("failed to read file"),
            "content"
        );
        assert_eq!(
            std::fs::read_to_string(target2.path().join("file.txt")).expect("failed to read file"),
            "content"
        );
    }

    #[test]
    fn test_deeply_nested_structure() {
        let (_store_tmpdir, store) = create_test_store();

        let source_tmpdir = TempDir::new().expect("failed to create source tmpdir");

        // Create a deeply nested path
        let deep_path = "a/b/c/d/e/f/g/h/i/j";
        std::fs::create_dir_all(source_tmpdir.path().join(deep_path))
            .expect("failed to create deep path");
        std::fs::write(
            source_tmpdir.path().join(format!("{}/deep.txt", deep_path)),
            "deep content",
        )
        .expect("failed to write deep file");

        let source_dir =
            fs::Dir::open_ambient_dir(source_tmpdir.path(), cap_std::ambient_authority())
                .expect("failed to open source dir");
        let root_checksum =
            add_dir_recursive(&store, source_dir).expect("failed to add source dir");

        let target_tmpdir = TempDir::new().expect("failed to create target tmpdir");
        let target_dir =
            fs::Dir::open_ambient_dir(target_tmpdir.path(), cap_std::ambient_authority())
                .expect("failed to open target dir");
        extract_dir_recursive(&store, &root_checksum, &target_dir).expect("failed to extract");

        let content =
            std::fs::read_to_string(target_tmpdir.path().join(format!("{}/deep.txt", deep_path)))
                .expect("failed to read deep file");
        assert_eq!(content, "deep content");
    }

    #[test]
    fn test_readonly_file_permissions() {
        let (_store_tmpdir, store) = create_test_store();

        let source_tmpdir = TempDir::new().expect("failed to create source tmpdir");
        let file_path = source_tmpdir.path().join("readonly.txt");
        std::fs::write(&file_path, "readonly content").expect("failed to write readonly file");

        let mut perms = std::fs::metadata(&file_path)
            .expect("failed to get metadata")
            .permissions();
        perms.set_mode(0o444);
        std::fs::set_permissions(&file_path, perms).expect("failed to set permissions");

        let source_dir =
            fs::Dir::open_ambient_dir(source_tmpdir.path(), cap_std::ambient_authority())
                .expect("failed to open source dir");
        let root_checksum =
            add_dir_recursive(&store, source_dir).expect("failed to add source dir");

        let target_tmpdir = TempDir::new().expect("failed to create target tmpdir");
        let target_dir =
            fs::Dir::open_ambient_dir(target_tmpdir.path(), cap_std::ambient_authority())
                .expect("failed to open target dir");
        extract_dir_recursive(&store, &root_checksum, &target_dir).expect("failed to extract");

        let target_meta = std::fs::metadata(target_tmpdir.path().join("readonly.txt"))
            .expect("failed to get meta");
        let mode = target_meta.permissions().mode() & 0o7777;
        assert_eq!(mode, 0o444, "readonly permissions should be preserved");
    }

    #[test]
    fn test_extract_hardlink_in_subdir_before_target() {
        // Reproduces the zoneinfo ordering bug: a hardlink inside a
        // subdirectory (e.g. Africa/Abidjan) that sorts alphabetically before
        // its canonical file at the parent level (e.g. UTC). The extraction
        // must create parent-level files before recursing into subdirectories.
        let (_store_tmpdir, store) = create_test_store();

        let source_tmpdir = TempDir::new().expect("failed to create source tmpdir");
        let base = source_tmpdir.path();

        // "zzz_canonical" sorts after "aaa_subdir/" alphabetically
        std::fs::write(base.join("zzz_canonical"), "shared content")
            .expect("failed to write canonical file");

        std::fs::create_dir(base.join("aaa_subdir")).expect("failed to create aaa_subdir");
        // Create a hardlink: aaa_subdir/link shares an inode with zzz_canonical
        std::fs::hard_link(base.join("zzz_canonical"), base.join("aaa_subdir/link"))
            .expect("failed to create hardlink");

        let source_dir = fs::Dir::open_ambient_dir(base, cap_std::ambient_authority())
            .expect("failed to open source dir");
        let root_checksum =
            add_dir_recursive(&store, source_dir).expect("failed to add source dir");

        let target_tmpdir = TempDir::new().expect("failed to create target tmpdir");
        let target_dir =
            fs::Dir::open_ambient_dir(target_tmpdir.path(), cap_std::ambient_authority())
                .expect("failed to open target dir");
        extract_dir_recursive(&store, &root_checksum, &target_dir)
            .expect("failed to extract: hardlink target should exist before subdir is processed");

        let target = target_tmpdir.path();
        assert_eq!(
            std::fs::read_to_string(target.join("zzz_canonical")).expect("failed to read file"),
            "shared content"
        );
        assert_eq!(
            std::fs::read_to_string(target.join("aaa_subdir/link")).expect("failed to read link"),
            "shared content",
            "hardlink content should match canonical file"
        );

        // Verify they share the same inode (are actual hardlinks)
        let canonical_ino = std::fs::metadata(target.join("zzz_canonical"))
            .expect("failed to get metadata")
            .ino();
        let link_ino = std::fs::metadata(target.join("aaa_subdir/link"))
            .expect("failed to get metadata")
            .ino();
        assert_eq!(
            canonical_ino, link_ino,
            "hardlink should share the same inode"
        );
    }
}
