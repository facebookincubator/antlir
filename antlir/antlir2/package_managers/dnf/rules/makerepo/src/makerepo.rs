/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fs::File;
use std::io::BufReader;
use std::io::BufWriter;
use std::io::Read;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use clap::Parser;
use clap::ValueEnum;
use digest::Digest;
use flate2::write::GzEncoder;
use quick_xml::events::BytesEnd;
use quick_xml::events::BytesStart;
use quick_xml::events::BytesText;
use quick_xml::events::Event;
use quick_xml::Writer as XmlWriter;
use serde::Deserialize;
use sha2::digest;
use sha2::Sha256;

#[derive(Debug, Parser)]
struct Args {
    #[clap(long)]
    repo_id: String,
    #[clap(long)]
    timestamp: Option<u64>,
    #[clap(long)]
    xml_dir: PathBuf,
    #[clap(long, value_enum, default_value_t)]
    compress: Compress,
    #[clap(long)]
    module_md: Option<PathBuf>,
    #[clap(long)]
    out: PathBuf,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Default, ValueEnum)]
enum Compress {
    #[default]
    None,
    Gzip,
}

struct DigestWriter<W: Write, D: Digest> {
    inner: W,
    hasher: D,
    len: u64,
}

impl<W: Write, D: Digest> DigestWriter<W, D> {
    fn new(inner: W) -> Self {
        Self {
            inner,
            hasher: D::new(),
            len: 0,
        }
    }

    fn finish(self) -> (W, digest::Output<D>, u64) {
        (self.inner, self.hasher.finalize(), self.len)
    }
}

impl<W: Write, D: Digest> Write for DigestWriter<W, D> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let written = self.inner.write(buf)?;
        self.hasher.update(&buf[..written]);
        self.len += written as u64;
        Ok(written)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

#[derive(Debug, Deserialize)]
struct PackageXmlBlobs {
    primary: String,
    filelists: String,
    other: String,
}

struct XmlFile<W: Write, D: Digest> {
    filename: String,
    element: &'static str,
    inner: XmlFileInner<W, D>,
}

enum XmlFileInner<W: Write, D: Digest> {
    Gzipped(DigestWriter<GzEncoder<DigestWriter<W, D>>, D>),
    Uncompressed(DigestWriter<W, D>),
}

impl XmlFile<BufWriter<File>, Sha256> {
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
        let w = BufWriter::new(f);
        let mut inner = match compress {
            Compress::None => XmlFileInner::Uncompressed(DigestWriter::new(w)),
            Compress::Gzip => XmlFileInner::Gzipped(DigestWriter::new(GzEncoder::new(
                DigestWriter::new(w),
                flate2::Compression::default(),
            ))),
        };
        inner.write_all(b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n")?;
        let element = match basename {
            "primary" => "metadata",
            "filelists" => "filelists",
            "other" => "otherdata",
            _ => unreachable!("only these three file names exist"),
        };
        let mut xml = XmlWriter::new_with_indent(inner, b' ', 2);
        let mut start = BytesStart::borrowed_name(element.as_bytes());
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
}

impl<W: Write, D: Digest> XmlFile<W, D> {
    fn write_package(&mut self, package: &str) -> std::io::Result<()> {
        match &mut self.inner {
            XmlFileInner::Gzipped(w) => w.write_all(package.as_bytes()),
            XmlFileInner::Uncompressed(w) => w.write_all(package.as_bytes()),
        }
    }

    fn finish(self) -> Result<RepomdRecord> {
        let mut xml = XmlWriter::new(self.inner);
        xml.write_event(Event::End(BytesEnd::borrowed(self.element.as_bytes())))?;
        let inner = xml.into_inner();
        match inner {
            XmlFileInner::Gzipped(w) => {
                // The outer layer (and the first finish()) is the uncompressed
                // stream, so contains open-size and open-checksum
                let (w, open_checksum, open_len) = w.finish();
                // The next layer is the GzEncoder, which when finish()ed will
                // give us back the bottom DigestWriter that has the checksum
                // and size of the compressed data
                let (_, compressed_checksum, compressed_len) =
                    w.finish().context("while finishing compression")?.finish();
                Ok(RepomdRecord {
                    location: format!("repodata/{}", self.filename),
                    checksum: hex::encode(compressed_checksum),
                    size: compressed_len,
                    open_checksum: Some(hex::encode(open_checksum)),
                    open_size: Some(open_len),
                })
            }
            XmlFileInner::Uncompressed(w) => {
                let (_, checksum, len) = w.finish();
                Ok(RepomdRecord {
                    location: format!("repodata/{}", self.filename),
                    checksum: hex::encode(checksum),
                    size: len,
                    open_checksum: None,
                    open_size: None,
                })
            }
        }
    }
}

impl<W: Write, D: Digest> Write for XmlFileInner<W, D> {
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
    open_checksum: Option<String>,
    size: u64,
    open_size: Option<u64>,
}

impl RepomdRecord {
    fn write<W: Write>(&self, w: &mut XmlWriter<W>, timestamp: u64) -> quick_xml::Result<()> {
        w.create_element("checksum")
            .with_attribute(("type", "sha256"))
            .write_text_content(BytesText::from_plain_str(&self.checksum))?;
        if let Some(open_checksum) = &self.open_checksum {
            w.create_element("open-checksum")
                .with_attribute(("type", "sha256"))
                .write_text_content(BytesText::from_plain_str(open_checksum))?;
        }
        w.create_element("location")
            .with_attribute(("href", self.location.as_str()))
            .write_empty()?;
        w.create_element("timestamp")
            .write_text_content(BytesText::from_plain_str(&timestamp.to_string()))?;
        w.create_element("size")
            .write_text_content(BytesText::from_plain_str(&self.size.to_string()))?;
        if let Some(open_size) = self.open_size {
            w.create_element("open-size")
                .write_text_content(BytesText::from_plain_str(&open_size.to_string()))?;
        }
        Ok(())
    }
}

fn main() -> Result<()> {
    let args = Args::parse();
    std::fs::create_dir(&args.out)
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

    for path in xml_paths {
        let package = std::fs::read_to_string(&path)
            .with_context(|| format!("while reading {}", path.display()))?;
        let blobs: PackageXmlBlobs = serde_json::from_str(&package)
            .with_context(|| format!("while parsing {}", path.display()))?;
        primary.write_package(&blobs.primary)?;
        filelists.write_package(&blobs.filelists)?;
        other.write_package(&blobs.other)?;
    }

    let primary = primary.finish()?;
    let filelists = filelists.finish()?;
    let other = other.finish()?;

    let modulemd = if let Some(modulemd) = &args.module_md {
        std::fs::copy(
            modulemd,
            args.out
                .join(modulemd.file_name().expect("must have filename")),
        )
        .context("while copying modulemd")?;
        let mut reader = BufReader::new(File::open(modulemd).context("while opening modulemd")?);
        let mut hasher = Sha256::new();
        let mut buffer = [0; 4096];
        loop {
            let count = reader.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            hasher.update(&buffer[..count]);
        }
        let checksum = hex::encode(hasher.finalize());
        Some(RepomdRecord {
            location: format!(
                "repodata/{}",
                modulemd
                    .file_name()
                    .expect("must have filename")
                    .to_str()
                    .expect("must be utf8")
            ),
            checksum,
            size: modulemd
                .metadata()
                .context("while statting modulemd")?
                .len(),
            open_checksum: None,
            open_size: None,
        })
    } else {
        None
    };

    let timestamp = args.timestamp.unwrap_or_else(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("no time travel pls")
            .as_secs()
    });

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
            w.create_element("revision")
                .write_text_content(BytesText::from_plain_str(timestamp.to_string().as_str()))?;
            w.create_element("data")
                .with_attribute(("type", "primary"))
                .write_inner_content(|w| primary.write(w, timestamp))?;
            w.create_element("data")
                .with_attribute(("type", "filelists"))
                .write_inner_content(|w| filelists.write(w, timestamp))?;
            w.create_element("data")
                .with_attribute(("type", "other"))
                .write_inner_content(|w| other.write(w, timestamp))?;
            if let Some(modulemd) = &modulemd {
                w.create_element("data")
                    .with_attribute(("type", "modules"))
                    .write_inner_content(|w| modulemd.write(w, timestamp))?;
            }
            Ok(())
        })?;
    let mut repomd = repomd.into_inner();
    repomd.write_all(b"\n")?;

    Ok(())
}
