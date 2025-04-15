/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#![feature(let_chains)]

use std::fs::File;
use std::io::BufReader;
use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;

use antlir2_compile::CompilerContext;
use antlir2_depgraph_if::Requirement;
use antlir2_depgraph_if::Validator;
use antlir2_depgraph_if::item::FileType;
use antlir2_depgraph_if::item::FsEntry;
use antlir2_depgraph_if::item::Item;
use antlir2_depgraph_if::item::ItemKey;
use antlir2_depgraph_if::item::Path as PathItem;
use antlir2_features::types::BuckOutSource;
use antlir2_features::types::PathInLayer;
use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use flate2::read::GzDecoder;
use serde::Deserialize;
use tar::Archive;
use tempfile::TempDir;
use tracing::warn;
use zstd::stream::read::Decoder as ZstdDecoder;

pub type Feature = Tarball;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Tarball {
    pub src: BuckOutSource,
    pub into_dir: PathInLayer,
    pub force_root_ownership: bool,
    pub strip_components: usize,
}

enum ArchiveReader<'a> {
    Uncompressed(BufReader<File>),
    Gz(GzDecoder<BufReader<File>>),
    Zstd(ZstdDecoder<'a, BufReader<BufReader<File>>>),
}

impl std::io::Read for ArchiveReader<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            ArchiveReader::Uncompressed(r) => r.read(buf),
            ArchiveReader::Gz(r) => r.read(buf),
            ArchiveReader::Zstd(r) => r.read(buf),
        }
    }
}

impl Tarball {
    #[tracing::instrument(err)]
    fn open_archive(&self) -> Result<Archive<ArchiveReader>> {
        let extension = self.src.extension().with_context(|| {
            format!(
                "archive must have extension, but got '{}'",
                self.src.display()
            )
        })?;
        let file = BufReader::new(
            File::open(&self.src)
                .with_context(|| format!("while opening {}", self.src.display()))?,
        );
        match extension.as_bytes() {
            b"tar" => Ok(Archive::new(ArchiveReader::Uncompressed(file))),
            b"gz" => Ok(Archive::new(ArchiveReader::Gz(GzDecoder::new(file)))),
            b"zst" | b"zstd" => Ok(Archive::new(ArchiveReader::Zstd(
                ZstdDecoder::new(file).expect("infallible since no dictionary is being used"),
            ))),
            _ => Err(anyhow!(
                "invalid tar extension: {}",
                extension.to_string_lossy(),
            )),
        }
    }
}

impl antlir2_depgraph_if::RequiresProvides for Tarball {
    fn provides(&self) -> Result<Vec<Item>, String> {
        let mut provides = Vec::new();
        let mut archive = self.open_archive().map_err(|e| format!("{e:#?}"))?;
        for entry in archive
            .entries()
            .context("while iterating over entries")
            .map_err(|e| e.to_string())?
        {
            let entry = entry
                .context("while iterating over entries")
                .map_err(|e| e.to_string())?;
            let path = self.into_dir.join(
                entry
                    .path()
                    .expect("infallible on linux")
                    .components()
                    .skip(self.strip_components)
                    .collect::<PathBuf>(),
            );
            if entry.header().entry_type().is_dir() {
                provides.push(Item::Path(PathItem::Entry(FsEntry {
                    path,
                    file_type: FileType::Directory,
                    mode: entry
                        .header()
                        .mode()
                        .context("mode field corrupted")
                        .map_err(|e| e.to_string())?,
                })));
            } else if entry.header().entry_type().is_symlink() {
                let target = entry
                    .link_name()
                    .context("while getting symlink target")
                    .map_err(|e| e.to_string())?
                    .context("entry is symlink but missing target")
                    .map_err(|e| e.to_string())?;
                provides.push(Item::Path(PathItem::Symlink {
                    link: path,
                    target: target.into_owned(),
                }));
            } else if let Some(file_type) = match entry.header().entry_type() {
                tar::EntryType::Regular => Some(FileType::File),
                tar::EntryType::Link => Some(FileType::File),
                tar::EntryType::Char => Some(FileType::CharDevice),
                tar::EntryType::Block => Some(FileType::BlockDevice),
                tar::EntryType::Fifo => Some(FileType::Fifo),
                _ => None,
            } {
                provides.push(Item::Path(PathItem::Entry(FsEntry {
                    path,
                    file_type,
                    mode: entry
                        .header()
                        .mode()
                        .context("mode field corrupted")
                        .map_err(|e| e.to_string())?,
                })));
            } else {
                warn!("ignoring entry '{}' with unknown file type", path.display());
            }
        }
        Ok(provides)
    }

    fn requires(&self) -> Result<Vec<Requirement>, String> {
        // It would be great to assert requirements on the users referenced in
        // this tarball, but tars almost never use the name of a user, and we
        // can't do anything meaningful with only a uid, so just let it slide...

        // For relative dest paths (or `/`), parent() could be the empty string
        if let Some(parent) = self.into_dir.parent()
            && !parent.as_os_str().is_empty()
        {
            Ok(vec![Requirement::ordered(
                ItemKey::Path(parent.to_owned()),
                Validator::FileType(FileType::Directory),
            )])
        } else {
            Ok(Default::default())
        }
    }
}

impl antlir2_compile::CompileFeature for Tarball {
    #[tracing::instrument(name = "tarball", skip(ctx), ret, err)]
    fn compile(&self, ctx: &CompilerContext) -> antlir2_compile::Result<()> {
        let mut archive = self.open_archive().context("while opening archive")?;
        archive.set_preserve_mtime(true);
        archive.set_preserve_permissions(true);
        archive.set_preserve_ownerships(!self.force_root_ownership);
        archive.set_unpack_xattrs(true);
        if self.strip_components == 0 {
            archive.unpack(ctx.dst_path(&self.into_dir)?)?;
        } else {
            // the 'tar' crate doesn't natively support strip_components, so we
            // just extract it to a temporary directory and move the contents
            // over
            let dst = TempDir::new_in(ctx.root_path())?;
            archive.unpack(dst.path())?;
            let mut first_dir = dst.path().to_owned();
            for _depth in 0..self.strip_components {
                let entries =
                    std::fs::read_dir(&first_dir)?.collect::<std::io::Result<Vec<_>>>()?;
                if entries.len() != 1 {
                    return Err(anyhow!(
                        "expected exactly one entry in {} after stripping components, but saw {:?}",
                        dst.path().display(),
                        entries.iter().map(|e| e.file_name()).collect::<Vec<_>>(),
                    )
                    .into());
                }
                first_dir.push(entries[0].file_name());
            }
            let final_dst = ctx.dst_path(&self.into_dir)?;
            std::fs::rename(&first_dir, &final_dst).with_context(|| {
                format!(
                    "while moving extracted tarball {} -> {}",
                    first_dir.display(),
                    final_dst.display(),
                )
            })?;
        }
        Ok(())
    }
}
