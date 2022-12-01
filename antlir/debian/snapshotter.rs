/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Duration;

use absolute_path::AbsolutePathBuf;
use anyhow::anyhow;
use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use blob_store::get_blob;
use blob_store::get_sha2_hash;
use blob_store::upload;
use blob_store::Blob;
use blob_store::DownloadDetails;
use blob_store::PackageBackend;
use blob_store::RateLimitedPackageBackend;
use blob_store::StoreFormat;
use bytes::Buf;
use bytes::Bytes;
use clap::Parser as ClapParser;
use fbinit::FacebookInit;
use find_root::find_repo_root;
use futures::future::try_join_all;
use governor::clock::DefaultClock;
use governor::state::InMemoryState;
use governor::state::NotKeyed;
use manifold_client::cpp_client::ClientOptionsBuilder;
use manifold_client::cpp_client::ManifoldCppClient;
use pest::Parser;
use pest_derive::Parser;
use reqwest::Url;
use slog::info;
use slog::o;
use slog::Drain;
use snapshotter_helpers::Architecture;
use snapshotter_helpers::Args;
use snapshotter_helpers::ANTLIR_SNAPSHOTS_BUCKET;
use snapshotter_helpers::API_KEY;
use xz2::read::XzDecoder;
use xz2::read::XzEncoder;

const REPO_SNAPSHOT_FILE: &str = "fbcode/bot_generated/antlir/debian/repo_snapshots.bzl";

#[derive(Parser)]
#[grammar_inline = "WHITESPACE = _{\" \" | SPACE_SEPARATOR}\
hash = @{ ASCII_HEX_DIGIT{32,64} }\
extra_description = {unicode_string~ NEWLINE}\
unicode_string = { (LETTER | NUMBER | PUNCTUATION | SYMBOL  )+ }\
size = { ASCII_DIGIT+ }\
file_index = { (NEWLINE ~ hash ~ size ~ unicode_string) }\
key = { (ASCII_ALPHA | ASCII_DIGIT | \"-\")+}\
release_value = { (file_index+ | unicode_string)}\
release_field = { (key ~ \":\" ~ release_value) ~ NEWLINE }\
package_field = {key ~ \":\" ~ unicode_string ~ NEWLINE}\
package = {(package_field | extra_description)+ ~ NEWLINE+}\
release_file = {\
    SOI ~\
    release_field+ ~\
    EOI\
}\
package_file = {\
	SOI ~\
    package*~\
    EOI \
}\
"]
pub struct RepoParser;

#[derive(Debug, Eq, Copy, PartialEq, Clone)]
struct HashSha256([u8; 32]);

#[derive(Debug, PartialEq)]
struct ReleaseIndex<'a> {
    hash: HashSha256,
    from_repo: &'a Repo,
    path: String,
}

#[derive(Debug, PartialEq)]
struct Package<'a> {
    name: String,
    architecture: Architecture,
    version: String,
    path: String,
    hash: HashSha256,
    extra_details: BTreeMap<String, String>,
    release_index: &'a ReleaseIndex<'a>,
}

#[derive(Debug, PartialEq)]
struct Repo {
    repo_root_url: reqwest::Url,
    distro: String,
    arch: Architecture,
}

#[derive(Debug, PartialEq)]
struct ReleaseFile<'a> {
    fields: BTreeMap<String, String>,
    indices: Vec<ReleaseIndex<'a>>,
}

struct PackageFile {
    content: Bytes,
    xz_encode: Bytes,
    path: String,
}

struct VersionedReleaseFile {
    content: Bytes,
    //TODO: Add a signature field
}

impl<'a> StoreFormat for &Package<'a> {
    fn store_format(&self) -> Result<DownloadDetails> {
        Ok(DownloadDetails {
            content: Blob::Url(
                self.release_index
                    .from_repo
                    .repo_root_url
                    .join(self.path.as_str())?,
            ),
            key: format!("SHA256:{}", hex::encode(&self.hash.0)),
            name: self.name.clone(),
            version: self.version.clone(),
        })
    }
}

impl StoreFormat for &PackageFile {
    fn store_format(&self) -> Result<DownloadDetails> {
        Ok(DownloadDetails {
            content: Blob::Blob(self.xz_encode.clone()),
            key: format!("SHA256:{}", get_sha2_hash(&self.xz_encode)),
            name: format!("PackageFile:{}", self.path),
            version: get_sha2_hash(&self.xz_encode),
        })
    }
}

impl StoreFormat for &VersionedReleaseFile {
    fn store_format(&self) -> Result<DownloadDetails> {
        Ok(DownloadDetails {
            content: Blob::Blob(self.content.clone()),
            key: format!("SHA256:{}", get_sha2_hash(&self.content)),
            name: format!("ReleaseFile: {}", get_sha2_hash(&self.content)),
            version: get_sha2_hash(&self.content),
        })
    }
}

impl Repo {
    fn new(repo_url: &str, distro: String, arch: String) -> Result<Repo> {
        Ok(Repo {
            repo_root_url: Url::parse(repo_url)
                .with_context(|| format!("invalid url {}", repo_url))?,
            distro,
            arch: Architecture::from_str(arch.as_str())?,
        })
    }
    async fn get_release_indices<'a>(
        &'a self,
        client: &reqwest::Client,
        logger: slog::Logger,
    ) -> Result<ReleaseFile<'a>> {
        let release_text = String::from_utf8(
            get_blob(
                self.repo_root_url
                    .join(&format!("dists/{}/Release", self.distro))
                    .context("failed to join")?,
                client,
                4,
                logger.new(o!("file" =>"Release")),
            )
            .await?
            .to_vec(),
        )?;
        self.parse_release_file(&release_text, &format!("{:?}", self.arch))
    }
    fn parse_release_file<'a>(&'a self, release_text: &str, arch: &str) -> Result<ReleaseFile<'a>> {
        let release_tokens = RepoParser::parse(Rule::release_file, release_text)
            .context("Error parsing Release File")?
            .next()
            .expect("parser produces this value refer: SOI");
        let mut release_indices: Vec<ReleaseIndex> = Vec::new();
        let mut release_file_fields = BTreeMap::new();
        for field in release_tokens
            .into_inner()
            .filter(|token| matches!(token.as_rule(), Rule::release_field))
        {
            let mut itr = field.into_inner();
            match itr
                .next()
                .expect("parser produces this value refer: field")
                .as_str()
            {
                "SHA256" => {
                    let value = itr
                        .next()
                        .expect("parser always produces see Rule: file_index");
                    let mut indices = value
                        .into_inner()
                        .map(|file| {
                            let mut inr = file.into_inner();
                            let hash = HashSha256(
                                hex::decode(inr.next().context("missing hash")?.as_str())?
                                    .try_into()
                                    .map_err(|_| anyhow!("invalid sha length"))?,
                            );
                            let _length =
                                inr.next().context("missing length")?.as_str().to_string();
                            let path = inr.next().context("missing path")?.as_str().to_string();
                            Ok(ReleaseIndex {
                                hash,
                                from_repo: self,
                                path,
                            })
                        })
                        .collect::<Result<Vec<ReleaseIndex>>>()?
                        .into_iter()
                        .filter(|ri| {
                            ri.path.ends_with("Packages.xz")
                                && ri.path.contains(&format!("-{}", arch.to_lowercase()))
                        })
                        .collect();

                    release_indices.append(&mut indices);
                }
                "MD5Sum" => (),
                "SHA1" => (),
                key => {
                    release_file_fields.insert(
                        key.to_string(),
                        itr.next()
                            .expect("parser produces this")
                            .as_str()
                            .to_string(),
                    );
                }
            }
        }
        Ok(ReleaseFile {
            fields: release_file_fields,
            indices: release_indices,
        })
    }
}
impl<'a> Package<'a> {
    fn try_from_iter<I: IntoIterator<Item = (String, String)>>(
        iter: I,
        from_release: &'a ReleaseIndex,
    ) -> Result<Self> {
        let mut name = None;
        let mut architecture = None;
        let mut version = None;
        let mut path = None;
        let mut hash = None;
        let mut extra: BTreeMap<String, String> = BTreeMap::new();
        for (key, value) in iter {
            match key.as_str() {
                "Package" => name = Some(value),
                "Architecture" => architecture = Some(Architecture::from_str(value.as_str())?),
                "Version" => version = Some(value),
                "Filename" => path = Some(value),
                "SHA256" => {
                    hash = Some(HashSha256(
                        hex::decode(&value)?
                            .try_into()
                            .map_err(|_| anyhow!("{} is not a valid sha256", value))?,
                    ))
                }
                _ => {
                    extra.insert(key, value);
                }
            }
        }
        Ok(Package {
            name: name.context("missing name")?,
            architecture: architecture.context("missing arch")?,
            version: version.context("missing arch")?,
            path: path.context("missing context")?,
            hash: hash.context("missing hash")?,
            release_index: from_release,
            extra_details: extra,
        })
    }
}

impl<'a> ReleaseIndex<'a> {
    fn parse_package_file<'b>(&'b self, package_content: &str) -> Result<Vec<Package<'b>>> {
        let packages = RepoParser::parse(Rule::package_file, package_content)
            .context("Package parse Error")?
            .next()
            .expect("parser produces this value rule: SOI");
        let pkgs = packages
            .into_inner()
            .filter(|pkg| matches!(pkg.as_rule(), Rule::package))
            .map(|pk| {
                let iter = pk
                    .into_inner()
                    .filter(|field| matches!(field.as_rule(), Rule::package_field))
                    .map(|field| {
                        let mut itr_pk = field.into_inner();
                        (
                            itr_pk
                                .next()
                                .expect("parser produces this value refer: field")
                                .as_str()
                                .to_string(),
                            itr_pk
                                .next()
                                .expect("parser produces this value refer: field")
                                .as_str()
                                .to_string(),
                        )
                    });
                Package::try_from_iter(iter, self)
            })
            .collect::<Result<Vec<Package>>>();
        pkgs
    }
    async fn get_packages<'b>(
        &'b self,
        client: &reqwest::Client,
        logger: slog::Logger,
    ) -> Result<Vec<Package<'b>>> {
        let package_path = self
            .from_repo
            .repo_root_url
            .join(&format!("dists/{}/{}", self.from_repo.distro, self.path))
            .context("failed to join")
            .expect("this value would be present");
        let xz_stream =
            get_blob(package_path, client, 4, logger.new(o!("file" => "Package"))).await?;
        let mut plain_stream = XzDecoder::new_multi_decoder(xz_stream.reader());
        let mut package_content = String::new();
        plain_stream.read_to_string(&mut package_content)?;
        self.parse_package_file(&package_content)
    }
}

impl PackageFile {
    fn new(pkgs: &Vec<Package>, path: String) -> Result<Self> {
        let mut content = String::new();
        for pkg in pkgs {
            content.push_str(
                format!(
                    "{}: {}\n{}: {}\n{}: {}\n{}: {}\n{}: {}\n",
                    "Package",
                    pkg.name.clone(),
                    "Architecture",
                    format!("{:?}", pkg.architecture).to_lowercase(),
                    "Version",
                    pkg.version.clone(),
                    "SHA256",
                    hex::encode(pkg.hash.0),
                    "Filename",
                    format!("{}/SHA256:{}", "deb", hex::encode(pkg.hash.0))
                )
                .as_str(),
            );
            for (key, value) in &pkg.extra_details {
                content.push_str(format!("{}: {}\n", key, value).as_str());
            }
            content.push('\n');
        }
        content.push('\n');
        let mut xz_read = XzEncoder::new(content.as_bytes(), 6);
        let mut buffer = Vec::new();
        xz_read.read_to_end(&mut buffer)?;
        let xz_encode = Bytes::from(buffer);

        Ok(PackageFile {
            content: Bytes::from(content),
            xz_encode,
            path,
        })
    }
}
fn normalize_packages_path(path: &Path) -> Result<PathBuf> {
    match path.ends_with("Packages.xz") {
        false => Err(anyhow!("not path to Packages.xz")),
        true => Ok(path.with_extension("")),
    }
}

impl VersionedReleaseFile {
    fn new(pkg_files: &Vec<PackageFile>, aux_hash_map: BTreeMap<String, String>) -> Result<Self> {
        let mut content = String::new();
        let mut sha256_hash_index = String::new();
        for (key, value) in aux_hash_map {
            content.push_str(format!("{}: {}\n", key.as_str(), value.as_str()).as_str());
        }
        for pkg in pkg_files {
            sha256_hash_index.push_str(
                format!(
                    " {} {:>16} {}\n",
                    get_sha2_hash(&pkg.xz_encode),
                    pkg.xz_encode.len(),
                    pkg.path
                )
                .as_str(),
            );
            sha256_hash_index.push_str(
                format!(
                    " {} {:>16} {}\n",
                    get_sha2_hash(&pkg.content),
                    pkg.content.len(),
                    normalize_packages_path(Path::new(&pkg.path))?.display()
                )
                .as_str(),
            );
        }
        content.push_str(format!("SHA256:\n{}", sha256_hash_index.as_str()).as_str());
        Ok(VersionedReleaseFile {
            content: Bytes::from(content),
            //TODO: sign the content
        })
    }
}

async fn snapshot(
    repo: &Repo,
    storage_backend: impl PackageBackend,
    read_rl: Option<governor::RateLimiter<NotKeyed, InMemoryState, DefaultClock>>,
    write_rl: Option<governor::RateLimiter<NotKeyed, InMemoryState, DefaultClock>>,
    write_throughput: Option<governor::RateLimiter<NotKeyed, InMemoryState, DefaultClock>>,
    logger: slog::Logger,
) -> Result<String> {
    let client = reqwest::ClientBuilder::new()
        .redirect(reqwest::redirect::Policy::limited(3))
        .pool_idle_timeout(None)
        .tcp_keepalive(Some(Duration::from_secs(3600)))
        .build()?;
    let package_index = repo.get_release_indices(&client, logger.clone()).await?;
    let rl_packagebackend =
        RateLimitedPackageBackend::new(read_rl, write_rl, write_throughput, storage_backend);
    let mut package_files = Vec::new();
    for index in package_index.indices {
        let index_logger = logger.new(o!("index_path"=> index.path.clone()));
        let pkg = index.get_packages(&client, index_logger.clone()).await?;
        let updated_pkg_file = PackageFile::new(&pkg, index.path.clone())?;

        try_join_all(pkg.iter().map(|deb| {
            upload(
                deb,
                &rl_packagebackend,
                &client,
                index_logger.new(o!("package" => deb.name.clone())),
            )
        }))
        .await?;
        upload(
            &updated_pkg_file,
            &rl_packagebackend,
            &client,
            index_logger.new(o!("PackageFile" => index.path.clone())),
        )
        .await?;
        package_files.push(updated_pkg_file);
    }
    let components = package_index
        .fields
        .get("Components")
        .context("cannot file components key in releasefile")?
        .clone();
    let release_file = VersionedReleaseFile::new(&package_files, package_index.fields)?;

    upload(
        &release_file,
        &rl_packagebackend,
        &client,
        logger.new(o!("Release" => get_sha2_hash(&release_file.content))),
    )
    .await?;

    Ok(format!(
        "SHA256:{} {}",
        get_sha2_hash(&release_file.content),
        components
    ))
}

#[fbinit::main]
async fn main(fb: FacebookInit) -> Result<()> {
    let args = Args::parse();
    let repo = Repo::new(&args.repourl, args.distro, args.arch)?;
    let manifold_client_opts = ClientOptionsBuilder::default()
        .api_key(API_KEY)
        .build()
        .map_err(Error::msg)?;
    let manifold_client =
        ManifoldCppClient::from_options(fb, ANTLIR_SNAPSHOTS_BUCKET, &manifold_client_opts)
            .map_err(Error::from)?;
    let decorator = slog_term::TermDecorator::new().build();
    let drain = slog_term::FullFormat::new(decorator).build().fuse();
    let drain = slog_async::Async::new(drain).build().fuse();

    let log = slog::Logger::root(drain, o!());
    let release_hash = snapshot(
        &repo,
        manifold_client,
        args.readqps,
        args.writeqps,
        args.writethroughput,
        log.new(o!("repo" => repo.distro.clone(), "architecture" => format!("{:?}", repo.arch))),
    )
    .await?;
    let release_hash = "test";
    let repo_root = find_repo_root(&AbsolutePathBuf::new(std::env::current_exe()?)?)?;
    commit_deb_snapshot::write_to_bot_generated(
        release_hash.to_string(),
        repo.distro,
        args.flavor,
        format!("{:?}", repo.arch).to_lowercase(),
        &repo_root.join(Path::new(REPO_SNAPSHOT_FILE)),
    )?;
    commit_deb_snapshot::commit()?;
    info!(log, ""; "release_hash" => release_hash);
    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;
    #[test]
    fn test_release_file_parse() {
        let release_file = include_str!("test_data/sample_Release");
        let repo = Repo::new(
            "http://us.archive.ubuntu.com/ubuntu/",
            "jammy".to_string(),
            "amd64".to_string(),
        )
        .unwrap();
        let pkgs = repo.parse_release_file(release_file, "amd64").unwrap();
        let expected_pkgs = vec![ReleaseIndex {
            hash: HashSha256(
                hex::decode("76858a337b1665561a256cea6f7ef32515517754e3c5e54c1895cf29e1b41884")
                    .unwrap()
                    .try_into()
                    .unwrap(),
            ),
            path: "main/binary-amd64/Packages.xz".to_string(),
            from_repo: &repo,
        }];
        assert_eq!(pkgs.indices, expected_pkgs);

        let pkgs = repo.parse_release_file(release_file, "arm64").unwrap();
        let expected_pkgs = vec![ReleaseIndex {
            hash: HashSha256(
                hex::decode("400a7084ebab29422e1bc9e5ee06b4558a8ccc42d59cc93e278358e00cee5ef8")
                    .unwrap()
                    .try_into()
                    .unwrap(),
            ),
            path: "main/binary-arm64/Packages.xz".to_string(),
            from_repo: &repo,
        }];
        assert_eq!(pkgs.indices, expected_pkgs);
    }
    #[test]
    fn test_package_file_parse() {
        let package_file = include_str!("test_data/sample_Package");
        let repo = Repo::new(
            "http://us.archive.ubuntu.com/ubuntu/",
            "jammy".to_string(),
            "amd64".to_string(),
        )
        .unwrap();
        let release_index = ReleaseIndex {
            hash: HashSha256(
                hex::decode("400a7084ebab29422e1bc9e5ee06b4558a8ccc42d59cc93e278358e00cee5ef8")
                    .unwrap()
                    .try_into()
                    .unwrap(),
            ),
            path: "main/binary-arm64/Packages.xz".to_string(),
            from_repo: &repo,
        };
        let packages = release_index.parse_package_file(package_file).unwrap();
        let expected_packages = vec![
            Package {
                name: "a11y-profile-manager".to_string(),
                architecture: Architecture::Amd64,
                version: "0.1.10-0ubuntu3".to_string(),
                path: "pool/main/a/a11y-profile-manager/a11y-profile-manager_0.1.10-0ubuntu3_amd64.deb".to_string(),
                hash: HashSha256(hex::decode("863d375123f65eb2bef53fbea936379275da9e9b63dc18ba378cb3a4adcc82eb").unwrap().try_into().unwrap()),
                release_index: &release_index,
                extra_details: BTreeMap::from([("Origin".to_string(), "Ubuntu".to_string())])

            },
            Package {
                name: "a11y-profile-manager-doc".to_string(),
                architecture: Architecture::All,
                version: "0.1.10-0ubuntu3".to_string(),
                path: "pool/main/a/a11y-profile-manager/a11y-profile-manager-doc_0.1.10-0ubuntu3_all.deb".to_string(),
                hash: HashSha256(hex::decode("ec5354e806deed621283cc5697300777969abf95b846800690de67ff0fda40e8").unwrap().try_into().unwrap()),
                release_index: &release_index,
                extra_details: BTreeMap::from([("Origin".to_string(), "Ubuntu".to_string())])
            },
        ];
        assert_eq!(packages, expected_packages);
    }
    #[test]
    fn test_plain_file_format() {
        assert_eq!(
            normalize_packages_path(Path::new("main/binary-amd64/Packages.xz"))
                .expect("should have valid value"),
            Path::new("main/binary-amd64/Packages")
        )
    }
}
