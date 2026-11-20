/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::io::BufRead;
use std::io::BufReader;
use std::io::BufWriter;
use std::io::Write;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use clap::Parser;

/// Invert a Debian `Contents` index into a soname -> providing packages map.
///
/// An rpm declares the sonames it ships as `Provides`, so dnf can answer
/// "what provides libfoo.so.1" straight from the package index. Debian has no
/// such field; the archive-wide file list is the only published statement of
/// which package installs a given file, so the mapping is inverted once here
/// instead of being queried per lookup. The full index is ~170MiB of text,
/// while the shared libraries in it are a few MiB.
#[derive(Parser, Debug)]
pub(crate) struct ParseContents {
    /// Path to Contents index file, gzipped or not. Snapshots keep the blob
    /// exactly as the archive published it, so the encoding is not known until
    /// the file is opened.
    contents: PathBuf,
    /// Output path (or - for stdout)
    out: PathBuf,
}

/// Directories on the default loader search path. A library anywhere else
/// (`usr/lib/firefox/`, `usr/lib/debug/`, ...) is private to whatever ships it
/// and cannot satisfy another package's `DT_NEEDED`.
fn is_default_library_path(dir: &str) -> bool {
    match dir {
        "lib" | "lib64" | "usr/lib" | "usr/lib64" => true,
        _ => match dir.rsplit_once('/') {
            // multiarch, e.g. usr/lib/x86_64-linux-gnu
            Some((parent, tuple)) => {
                matches!(parent, "lib" | "usr/lib") && tuple.contains("-linux-")
            }
            None => false,
        },
    }
}

fn soname(path: &str) -> Option<&str> {
    let (dir, name) = path.rsplit_once('/')?;
    if !is_default_library_path(dir) {
        return None;
    }
    (name.contains(".so.") || name.ends_with(".so")).then_some(name)
}

fn parse<R: BufRead>(reader: R) -> Result<BTreeMap<String, BTreeSet<String>>> {
    let mut out: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for line in reader.lines() {
        let line = line.context("reading line")?;
        // A file name may contain spaces, so the package list is whatever
        // follows the last run of whitespace. Lines that do not split (and the
        // optional free-form header) never look like a library path.
        let Some((path, locations)) = line.rsplit_once(char::is_whitespace) else {
            continue;
        };
        let Some(soname) = soname(path.trim_end()) else {
            continue;
        };
        for location in locations.split(',') {
            // location is `[<area>/]<section>/<package>`
            let (section, package) = match location.rsplit_once('/') {
                Some(split) => split,
                None => continue,
            };
            // udebs only exist inside the debian installer
            if section.ends_with("debian-installer") {
                continue;
            }
            out.entry(soname.to_owned())
                .or_default()
                .insert(package.to_owned());
        }
    }
    Ok(out)
}

fn maybe_gunzip<'a, R: BufRead + 'a>(mut reader: R) -> Result<Box<dyn BufRead + 'a>> {
    if reader.fill_buf()?.starts_with(&[0x1f, 0x8b]) {
        Ok(Box::new(BufReader::new(flate2::read::GzDecoder::new(
            reader,
        ))))
    } else {
        Ok(Box::new(reader))
    }
}

impl ParseContents {
    #[tracing::instrument(err)]
    pub(crate) fn run(&self) -> Result<()> {
        let infile = maybe_gunzip(BufReader::new(stdio_path::open(&self.contents)?))
            .with_context(|| format!("while opening {}", self.contents.display()))?;
        let out =
            parse(infile).with_context(|| format!("while parsing {}", self.contents.display()))?;

        let mut outfile = BufWriter::new(stdio_path::create(&self.out)?);
        serde_json::to_writer(&mut outfile, &out)?;
        outfile.write_all(b"\n")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    fn parse_str(input: &str) -> BTreeMap<String, BTreeSet<String>> {
        parse(Cursor::new(input)).expect("failed to parse")
    }

    #[test]
    fn test_parse() {
        let out = parse_str(include_str!("../../../../testdata/deb/Contents-amd64"));
        assert_eq!(
            out["libc.so.6"],
            BTreeSet::from(["libc6".to_owned()]),
            "glibc's soname is not derivable from its package name",
        );
        assert_eq!(
            out["libcrypto.so.3"],
            BTreeSet::from(["libssl3t64".to_owned()]),
            "one package ships both halves of openssl",
        );
        assert_eq!(
            out["libjsoncpp.so.26"],
            BTreeSet::from(["libjsoncpp26".to_owned()]),
        );
        assert_eq!(
            out["libunwind.so.8"],
            BTreeSet::from(["libunwind8".to_owned()]),
        );
        assert_eq!(
            out["libsystemd.so.0"],
            BTreeSet::from(["libsystemd0".to_owned()]),
        );
        // Interchangeable builds of the same library genuinely both provide it
        assert_eq!(
            out["libQt5Gui.so.5"],
            BTreeSet::from(["libqt5gui5-gles".to_owned(), "libqt5gui5t64".to_owned()]),
        );
    }

    #[test]
    fn test_skips_unlinkable_libraries() {
        let out = parse_str(include_str!("../../../../testdata/deb/Contents-amd64"));
        assert!(
            !out.contains_key("libxul.so"),
            "libraries in an application's private directory are not linkable",
        );
        assert!(
            !out.contains_key("libblas.so.3"),
            "alternatives live in a subdirectory that is not on the loader path",
        );
        assert!(
            !out.contains_key("libdebconf.so.0"),
            "udebs never provide a runtime library",
        );
        assert!(
            !out.contains_key("libc.so.6.debug"),
            "usr/lib/debug is not on the loader path",
        );
    }

    #[test]
    fn test_parse_empty() {
        assert!(parse_str("").is_empty());
    }
}
