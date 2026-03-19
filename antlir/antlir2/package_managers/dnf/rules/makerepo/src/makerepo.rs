/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fs::File;
use std::io::BufWriter;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use anyhow::ensure;
use clap::Parser;
use clap::ValueEnum;
use flate2::GzBuilder;
use flate2::write::GzEncoder;
use quick_xml::Writer as XmlWriter;
use quick_xml::events::BytesEnd;
use quick_xml::events::BytesStart;
use quick_xml::events::Event;
use serde::Deserialize;
use sha2::Digest;
use sha2::Sha256;

#[derive(Debug, Parser)]
struct Args {
    #[clap(long)]
    repo_id: String,
    #[clap(long)]
    xml_dir: PathBuf,
    #[clap(long, value_enum, default_value_t)]
    compress: Compress,
    #[clap(long)]
    module_md: Option<PathBuf>,
    #[clap(long)]
    out: PathBuf,
    #[clap(long)]
    expected_rpm_count: Option<u32>,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Default, ValueEnum)]
enum Compress {
    #[default]
    None,
    Gzip,
}

#[derive(Debug, Deserialize)]
struct PackageXmlBlobs {
    primary: String,
    filelists: String,
    other: String,
}

struct HashingWriter<W: Write> {
    inner: W,
    hasher: Sha256,
}

impl<W: Write> HashingWriter<W> {
    fn new(inner: W) -> Self {
        Self {
            inner,
            hasher: Sha256::new(),
        }
    }

    fn finalize(self) -> (W, String) {
        let digest = self.hasher.finalize();
        (self.inner, hex::encode(digest))
    }
}

impl<W: Write> Write for HashingWriter<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let n = self.inner.write(buf)?;
        self.hasher.update(&buf[..n]);
        Ok(n)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

struct XmlFile<W: Write> {
    filename: String,
    element: &'static str,
    inner: HashingWriter<XmlFileInner<W>>,
}

enum XmlFileInner<W: Write> {
    Gzipped(GzEncoder<W>),
    Uncompressed(W),
}

impl XmlFile<HashingWriter<BufWriter<File>>> {
    fn new(
        basename: &str,
        num_packages: usize,
        out_dir: &Path,
        compress: Compress,
    ) -> Result<Self> {
        let filename = match compress {
            Compress::None => format!("{}.xml", basename),
            Compress::Gzip => format!("{}.xml.gz", basename),
        };
        let path = out_dir.join(&filename);
        let f =
            File::create(&path).with_context(|| format!("while creating {}", path.display()))?;
        let w = HashingWriter::new(BufWriter::new(f));
        let inner = match compress {
            Compress::None => XmlFileInner::Uncompressed(w),
            Compress::Gzip => XmlFileInner::Gzipped(
                GzBuilder::new()
                    .mtime(0) // deterministic output
                    .write(w, flate2::Compression::default()),
            ),
        };
        let mut inner = HashingWriter::new(inner);
        inner.write_all(b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n")?;
        let element = match basename {
            "primary" => "metadata",
            "filelists" => "filelists",
            "other" => "otherdata",
            _ => unreachable!("only these three file names exist"),
        };
        let mut xml = XmlWriter::new_with_indent(inner, b' ', 2);
        let mut start = BytesStart::new(element);
        match basename {
            "primary" => {
                start.push_attribute(("xmlns", "http://linux.duke.edu/metadata/common"));
                start.push_attribute(("xmlns:rpm", "http://linux.duke.edu/metadata/rpm"));
            }
            "filelists" => {
                start.push_attribute(("xmlns", "http://linux.duke.edu/metadata/filelists"));
            }
            "other" => {
                start.push_attribute(("xmlns", "http://linux.duke.edu/metadata/other"));
            }
            _ => unreachable!("only these three file names exist"),
        }
        start.push_attribute(("packages", num_packages.to_string().as_str()));
        xml.write_event(Event::Start(start))?;
        let mut inner = xml.into_inner();
        inner.write_all(b"\n")?;
        Ok(Self {
            filename,
            element,
            inner,
        })
    }

    fn finish(self) -> Result<RepomdRecord> {
        let mut xml = XmlWriter::new(self.inner);
        xml.write_event(Event::End(BytesEnd::new(self.element)))?;
        let open_hashing_writer = xml.into_inner();
        let (xml_file_inner, open_checksum) = open_hashing_writer.finalize();
        let hashing_writer = match xml_file_inner {
            XmlFileInner::Gzipped(w) => w.finish().context("while finishing gzip stream")?,
            XmlFileInner::Uncompressed(w) => w,
        };
        let (mut buf_writer, checksum) = hashing_writer.finalize();
        buf_writer.flush().context("while flushing xml")?;
        let file = buf_writer
            .into_inner()
            .context("while unwrapping xml buf writer")?;
        file.sync_all().context("while syncing xml")?;

        Ok(RepomdRecord {
            location: format!("repodata/{}", self.filename),
            checksum,
            open_checksum,
        })
    }
}

impl<W: Write> XmlFile<W> {
    fn write_package(&mut self, package: &str) -> std::io::Result<()> {
        self.inner.write_all(package.as_bytes())
    }
}

impl<W: Write> Write for XmlFileInner<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            Self::Gzipped(w) => w.write(buf),
            Self::Uncompressed(w) => w.write(buf),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Self::Gzipped(w) => w.flush(),
            Self::Uncompressed(w) => w.flush(),
        }
    }
}

struct RepomdRecord {
    location: String,
    checksum: String,
    open_checksum: String,
}

impl RepomdRecord {
    fn write<W: Write>(&self, w: &mut XmlWriter<W>) -> quick_xml::Result<()> {
        w.create_element("checksum")
            .with_attribute(("type", "sha256"))
            .write_text_content(quick_xml::events::BytesText::new(&self.checksum))?;
        w.create_element("open-checksum")
            .with_attribute(("type", "sha256"))
            .write_text_content(quick_xml::events::BytesText::new(&self.open_checksum))?;
        w.create_element("location")
            .with_attribute(("href", self.location.as_str()))
            .write_empty()?;
        Ok(())
    }
}

/// Read info about individual rpm files from `xml_dir` and build repodata in the `out` directory.
fn main() -> Result<()> {
    let args = Args::parse();
    std::fs::create_dir_all(&args.out)
        .with_context(|| format!("while creating output dir {}", args.out.display()))?;
    let mut xml_paths: Vec<PathBuf> = std::fs::read_dir(&args.xml_dir)
        .with_context(|| format!("while listing files in {}", args.xml_dir.display()))?
        .map(|e| e.map(|e| e.path()).map_err(anyhow::Error::from))
        .collect::<Result<_>>()
        .with_context(|| format!("while listing files in {}", args.xml_dir.display()))?;
    xml_paths.sort();
    let xml_paths = xml_paths;

    let mut primary = XmlFile::new("primary", xml_paths.len(), &args.out, args.compress)?;
    let mut filelists = XmlFile::new("filelists", xml_paths.len(), &args.out, args.compress)?;
    let mut other = XmlFile::new("other", xml_paths.len(), &args.out, args.compress)?;
    let rpm_count = xml_paths.len() as u32;

    for path in xml_paths {
        let package = std::fs::read_to_string(&path)
            .with_context(|| format!("while reading {}", path.display()))?;
        let blobs: PackageXmlBlobs = serde_json::from_str(&package)
            .with_context(|| format!("while parsing {}", path.display()))?;
        primary.write_package(&blobs.primary)?;
        filelists.write_package(&blobs.filelists)?;
        other.write_package(&blobs.other)?;
    }

    if let Some(expected_rpm_count) = args.expected_rpm_count {
        ensure!(
            expected_rpm_count == rpm_count,
            "Expected rpm count {} doesn't match real one {}.",
            expected_rpm_count,
            rpm_count,
        );
    }

    let primary = primary.finish()?;
    let filelists = filelists.finish()?;
    let other = other.finish()?;

    let modulemd = if let Some(modulemd) = &args.module_md {
        let data = std::fs::read(modulemd).context("while reading modulemd")?;
        let checksum = hex::encode(Sha256::digest(&data));
        std::fs::write(
            args.out
                .join(modulemd.file_name().expect("must have filename")),
            &data,
        )
        .context("while writing modulemd")?;
        Some(RepomdRecord {
            location: format!(
                "repodata/{}",
                modulemd
                    .file_name()
                    .expect("must have filename")
                    .to_str()
                    .expect("must be utf8")
            ),
            open_checksum: checksum.clone(),
            checksum,
        })
    } else {
        None
    };

    let mut inner = BufWriter::new(
        File::create(args.out.join("repomd.xml")).context("while creating repomd.xml")?,
    );
    inner.write_all(b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n")?;
    let mut repomd = XmlWriter::new_with_indent(inner, b' ', 2);
    repomd
        .create_element("repomd")
        .with_attribute(("xmlns", "http://linux.duke.edu/metadata/repo"))
        .with_attribute(("xmlns:rpm", "http://linux.duke.edu/metadata/rpm"))
        .write_inner_content(|w| {
            w.create_element("data")
                .with_attribute(("type", "primary"))
                .write_inner_content(|w| primary.write(w).map_err(std::io::Error::other))?;
            w.create_element("data")
                .with_attribute(("type", "filelists"))
                .write_inner_content(|w| filelists.write(w).map_err(std::io::Error::other))?;
            w.create_element("data")
                .with_attribute(("type", "other"))
                .write_inner_content(|w| other.write(w).map_err(std::io::Error::other))?;
            if let Some(modulemd) = &modulemd {
                w.create_element("data")
                    .with_attribute(("type", "modules"))
                    .write_inner_content(|w| modulemd.write(w).map_err(std::io::Error::other))?;
            }
            Ok(())
        })?;
    let mut repomd = repomd.into_inner();
    repomd.write_all(b"\n")?;
    repomd.flush().context("while flushing repomd.xml")?;
    let repomd = repomd
        .into_inner()
        .context("while unwrapping repomd.xml buf writer")?;
    repomd.sync_all().context("while syncing repomd.xml")?;

    Ok(())
}
