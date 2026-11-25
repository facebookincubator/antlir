/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Explains an `EACCES` from spawning the isolated program, which on its own
//! names neither the file nor the reason.

use std::ffi::OsStr;
use std::fs::OpenOptions;
use std::io::ErrorKind;
use std::io::Read;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::path::PathBuf;

use goblin::container::Ctx;
use goblin::elf::Elf;
use goblin::elf::ProgramHeader;
use goblin::elf::program_header::PT_INTERP;
use nix::fcntl::OFlag;
use nix::unistd::AccessFlags;
use nix::unistd::access;

/// How much of a program to read for its interpreter, which `PT_INTERP`
/// places right after the ELF and program headers
const ELF_HEAD_BYTES: u64 = 64 * 1024;

/// Describe the files `execve` checks when running `program` (looked up in
/// `path_env` if it has no slash): each candidate's mode and owner, a missing
/// execute bit, a parent directory that cannot be searched, and the same for
/// an ELF program's interpreter.
pub(crate) fn explain(program: &OsStr, path_env: &OsStr) -> String {
    let candidates: Vec<PathBuf> = if program.as_bytes().contains(&b'/') {
        vec![PathBuf::from(program)]
    } else {
        // a candidate in a directory that cannot be searched is what execvp
        // reports EACCES for, so keep it
        std::env::split_paths(path_env)
            .map(|dir| dir.join(program))
            .filter(|path| match path.symlink_metadata() {
                Ok(_) => true,
                Err(e) => e.kind() == ErrorKind::PermissionDenied,
            })
            .collect()
    };
    if candidates.is_empty() {
        return format!("{} not found in PATH", Path::new(program).display());
    }
    candidates
        .iter()
        .map(|path| {
            let mut description = describe(path);
            if let Some(interpreter) = elf_interpreter(path) {
                description.push_str(&format!(
                    ", ELF interpreter {}",
                    describe(Path::new(&interpreter))
                ));
            }
            description
        })
        .collect::<Vec<_>>()
        .join("; ")
}

fn describe(path: &Path) -> String {
    let mut description = match path.metadata() {
        Ok(metadata) => {
            let mut description = format!(
                "{} (mode {:04o}, owner {}:{})",
                path.display(),
                metadata.mode() & 0o7777,
                metadata.uid(),
                metadata.gid(),
            );
            if !metadata.is_file() {
                description.push_str(" is not a regular file");
            } else if metadata.mode() & 0o111 == 0 {
                description.push_str(" has no execute bit");
            }
            description
        }
        Err(e) => format!("{}: {e}", path.display()),
    };
    if let Some(dir) = path
        .ancestors()
        .skip(1)
        .filter(|dir| !dir.as_os_str().is_empty())
        .find(|dir| access(*dir, AccessFlags::X_OK).is_err())
    {
        description.push_str(&format!(", directory {} is not searchable", dir.display()));
    }
    description
}

fn elf_interpreter(path: &Path) -> Option<String> {
    // Without O_NONBLOCK opening a FIFO waits for a writer; execve refuses
    // anything but a regular file, so nothing else has an interpreter
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(OFlag::O_NONBLOCK.bits())
        .open(path)
        .ok()?;
    if !file.metadata().ok()?.is_file() {
        return None;
    }
    let mut head = Vec::new();
    file.take(ELF_HEAD_BYTES).read_to_end(&mut head).ok()?;

    let header = Elf::parse_header(&head).ok()?;
    let ctx = Ctx::new(header.container().ok()?, header.endianness().ok()?);
    let program_headers = ProgramHeader::parse(
        &head,
        usize::try_from(header.e_phoff).ok()?,
        usize::from(header.e_phnum),
        ctx,
    )
    .ok()?;
    let interp = program_headers.iter().find(|ph| ph.p_type == PT_INTERP)?;
    let start = usize::try_from(interp.p_offset).ok()?;
    let end = start.checked_add(usize::try_from(interp.p_filesz).ok()?)?;
    let bytes = head.get(start..end)?;
    let name = bytes.split(|b| *b == 0).next()?;
    Some(String::from_utf8_lossy(name).into_owned())
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    fn file_with_mode(dir: &Path, name: &str, contents: &[u8], mode: u32) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, contents).expect("failed to write file");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode))
            .expect("failed to set mode");
        path
    }

    #[test]
    fn names_a_missing_execute_bit() {
        let dir = tempfile::tempdir().expect("failed to create tempdir");
        let program = file_with_mode(dir.path(), "tool", b"#!/bin/sh\n", 0o444);
        let explanation = explain(program.as_os_str(), OsStr::new(""));
        assert!(
            explanation.contains("(mode 0444,") && explanation.contains("has no execute bit"),
            "{explanation}"
        );
    }

    #[test]
    fn looks_up_a_bare_name_in_path() {
        let empty = tempfile::tempdir().expect("failed to create tempdir");
        let dir = tempfile::tempdir().expect("failed to create tempdir");
        let program = file_with_mode(dir.path(), "tool", b"#!/bin/sh\n", 0o444);
        let path_env = std::env::join_paths([empty.path(), dir.path()]).expect("valid PATH");
        let explanation = explain(OsStr::new("tool"), &path_env);
        assert!(
            explanation.starts_with(&program.display().to_string())
                && explanation.contains("has no execute bit"),
            "only the directory that has the tool is described: {explanation}"
        );
    }

    #[test]
    fn executable_file_is_not_blamed() {
        let dir = tempfile::tempdir().expect("failed to create tempdir");
        let program = file_with_mode(dir.path(), "tool", b"#!/bin/sh\n", 0o755);
        let explanation = explain(program.as_os_str(), OsStr::new(""));
        assert!(
            !explanation.contains("has no execute bit") && !explanation.contains("not searchable"),
            "{explanation}"
        );
    }

    #[test]
    fn describes_the_elf_interpreter() {
        let self_exe = std::env::current_exe().expect("no current exe");
        let contents = std::fs::read(&self_exe).expect("failed to read current exe");
        let parsed = Elf::parse(&contents).expect("current exe is not ELF");
        assert_eq!(
            elf_interpreter(&self_exe).as_deref(),
            parsed.interpreter,
            "the interpreter read from the head of the file is the one the whole file names"
        );
        let Some(interpreter) = parsed.interpreter else {
            // a statically linked test binary has no interpreter to describe
            return;
        };
        let explanation = explain(self_exe.as_os_str(), OsStr::new(""));
        assert!(
            explanation.contains(&format!("ELF interpreter {interpreter} (mode")),
            "{explanation}"
        );
    }

    #[test]
    fn a_fifo_is_not_read() {
        let dir = tempfile::tempdir().expect("failed to create tempdir");
        let fifo = dir.path().join("fifo");
        nix::unistd::mkfifo(&fifo, nix::sys::stat::Mode::from_bits_truncate(0o755))
            .expect("failed to create fifo");
        let explanation = explain(fifo.as_os_str(), OsStr::new(""));
        assert!(
            explanation.contains("is not a regular file") && !explanation.contains("ELF"),
            "returns without waiting for a writer: {explanation}"
        );
    }

    #[test]
    fn names_an_unsearchable_path_directory() {
        if nix::unistd::geteuid().is_root() {
            // root searches any directory
            return;
        }
        let dir = tempfile::tempdir().expect("failed to create tempdir");
        let bin = dir.path().join("bin");
        std::fs::create_dir(&bin).expect("failed to create bin");
        file_with_mode(&bin, "tool", b"#!/bin/sh\n", 0o755);
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o600))
            .expect("failed to set mode");
        let path_env = std::env::join_paths([&bin]).expect("valid PATH");
        let explanation = explain(OsStr::new("tool"), &path_env);
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755))
            .expect("failed to restore mode");
        assert!(
            explanation.contains(&format!("directory {} is not searchable", bin.display())),
            "{explanation}"
        );
    }

    #[test]
    fn reports_a_program_not_in_path() {
        let dir = tempfile::tempdir().expect("failed to create tempdir");
        let path_env = std::env::join_paths([dir.path()]).expect("valid PATH");
        assert_eq!(
            explain(OsStr::new("missing"), &path_env),
            "missing not found in PATH"
        );
    }
}
