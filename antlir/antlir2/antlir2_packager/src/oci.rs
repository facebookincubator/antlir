/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeSet;
use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::io::BufWriter;
use std::io::Read;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::str::FromStr;
use std::sync::Arc;

use antlir2_facts::Fact;
use antlir2_facts::Key;
use antlir2_facts::RoDatabase;
use antlir2_facts::fact_impl;
use anyhow::Context;
use anyhow::Result;
use cap_std::fs::Dir;
use maplit::hashmap;
use oci_spec::image::ANNOTATION_REF_NAME;
use oci_spec::image::Arch;
use oci_spec::image::ConfigBuilder;
use oci_spec::image::Descriptor;
use oci_spec::image::DescriptorBuilder;
use oci_spec::image::History;
use oci_spec::image::HistoryBuilder;
use oci_spec::image::ImageConfigurationBuilder;
use oci_spec::image::ImageIndexBuilder;
use oci_spec::image::ImageManifestBuilder;
use oci_spec::image::MediaType;
use oci_spec::image::OciLayoutBuilder;
use oci_spec::image::PlatformBuilder;
use oci_spec::image::RootFsBuilder;
use oci_spec::image::Sha256Digest;
use serde::Deserialize;
use serde::Serialize;
use sha2::Digest;
use sha2::Sha256;
use tempfile::tempdir_in;

use crate::run_cmd;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub struct OciLabel {
    key: String,
    value: String,
}

#[fact_impl("antlir2_packager::oci::OciLabel")]
impl Fact for OciLabel {
    fn key(&self) -> Key {
        // instead of just using the key, use the full key=value pair to be able
        // to see any conflicts later on
        format!("{}={}", self.key, self.value).into()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub struct OciEnv {
    key: String,
    value: String,
}

#[fact_impl("antlir2_packager::oci::OciEnv")]
impl Fact for OciEnv {
    fn key(&self) -> Key {
        // use the full KEY=VALUE pair to be able to see any conflicts later on
        format!("{}={}", self.key, self.value).into()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub struct OciExposedPort {
    port: String,
}

#[fact_impl("antlir2_packager::oci::OciExposedPort")]
impl Fact for OciExposedPort {
    fn key(&self) -> Key {
        self.port.clone().into()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub struct OciCmd {
    cmd: Vec<String>,
}

#[fact_impl("antlir2_packager::oci::OciCmd")]
impl Fact for OciCmd {
    fn key(&self) -> Key {
        self.cmd.join("\t").into()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub struct OciUser {
    user: String,
}

#[fact_impl("antlir2_packager::oci::OciUser")]
impl Fact for OciUser {
    fn key(&self) -> Key {
        self.user.clone().into()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub struct OciWorkingDir {
    working_dir: String,
}

#[fact_impl("antlir2_packager::oci::OciWorkingDir")]
impl Fact for OciWorkingDir {
    fn key(&self) -> Key {
        self.working_dir.clone().into()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub struct OciStopSignal {
    stop_signal: String,
}

#[fact_impl("antlir2_packager::oci::OciStopSignal")]
impl Fact for OciStopSignal {
    fn key(&self) -> Key {
        self.stop_signal.clone().into()
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Oci {
    deltas: Vec<Delta>,
    #[serde(rename = "ref")]
    refname: String,
    skopeo: PathBuf,
    target_arch: Arch,
    entrypoint: Vec<String>,
    facts_db: PathBuf,
    #[serde(default)]
    zstd_chunked: bool,
    #[serde(default)]
    base_layers_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Delta {
    tar: PathBuf,
    tar_zst: PathBuf,
    #[serde(default)]
    name: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct BaseLayersManifest {
    layers: Vec<Delta>,
    #[serde(default)]
    history: Vec<History>,
}

trait Blob {
    fn to_bytes(&self) -> Result<Arc<Vec<u8>>>;
}

trait OciObject: Blob {
    const MEDIA_TYPE: MediaType;
}

impl OciObject for oci_spec::image::ImageManifest {
    const MEDIA_TYPE: MediaType = MediaType::ImageManifest;
}

impl OciObject for oci_spec::image::ImageConfiguration {
    const MEDIA_TYPE: MediaType = MediaType::ImageConfig;
}

impl<T> Blob for T
where
    T: Serialize,
{
    fn to_bytes(&self) -> Result<Arc<Vec<u8>>> {
        serde_json::to_vec_pretty(&self)
            .context("while serializing object")
            .map(Arc::new)
    }
}

struct LayerTarZst(Arc<Vec<u8>>);

impl Blob for LayerTarZst {
    fn to_bytes(&self) -> Result<Arc<Vec<u8>>> {
        Ok(self.0.clone())
    }
}

impl OciObject for LayerTarZst {
    const MEDIA_TYPE: MediaType = MediaType::ImageLayerZstd;
}

/// Take some OCI object, write it to the blobs dir and return a descriptor
fn write<O: OciObject>(blobs_dir: &Dir, obj: &O) -> Result<Descriptor> {
    let bytes = obj.to_bytes().context("while serializing object")?;
    let sha256 = hex::encode(Sha256::digest(bytes.as_ref()));
    let mut f = BufWriter::new(
        blobs_dir
            .create(&sha256)
            .context("while creating blob file")?,
    );
    f.write_all(bytes.as_ref()).context("while writing blob")?;
    DescriptorBuilder::default()
        .media_type(O::MEDIA_TYPE)
        .digest(Sha256Digest::from_str(&sha256)?)
        .size(bytes.as_ref().len() as u64)
        .build()
        .context("while building descriptor")
}

impl Oci {
    pub(crate) fn build(&self, out: &Path) -> Result<()> {
        self.validate_target_arch()?;

        if self.zstd_chunked {
            self.build_zstd_chunked(out)
        } else {
            self.build_layout(out)
        }
    }

    fn validate_target_arch(&self) -> Result<()> {
        if let Arch::Other(arch) = &self.target_arch {
            anyhow::bail!("unsupported OCI target architecture '{arch}'; expected a GOARCH value");
        }
        Ok(())
    }

    fn build_zstd_chunked(&self, out: &Path) -> Result<()> {
        std::fs::create_dir_all(out).context("while creating output directory")?;
        let input_oci = tempdir_in(out).context("while creating temporary OCI directory")?;
        self.build_layout(input_oci.path())?;

        let src_oci_arg = format!("oci:{}:{}", input_oci.path().display(), self.refname);
        let dest_oci_arg = format!("oci:{}:{}", out.display(), self.refname);

        let mut cmd = Command::new(&self.skopeo);
        cmd.arg("--insecure-policy")
            .arg("copy")
            .arg("--dest-compress-format=zstd:chunked")
            .arg(src_oci_arg)
            .arg(dest_oci_arg);
        run_cmd(&mut cmd).context("while converting OCI layout to zstd:chunked")?;

        Ok(())
    }

    fn build_layout(&self, out: &Path) -> Result<()> {
        std::fs::create_dir_all(out).context("while creating output directory")?;
        let out = Dir::open_ambient_dir(out, cap_std::ambient_authority())
            .context("while opening output dir")?;

        let layout = OciLayoutBuilder::default()
            .image_layout_version("1.0.0")
            .build()
            .context("while building oci-layout")?;
        layout
            .to_writer_pretty(&mut BufWriter::new(
                out.create("oci-layout")
                    .context("while creating oci-layout")?,
            ))
            .context("while writing oci-layout")?;

        out.create_dir_all("blobs/sha256")
            .context("while creating blobs dir")?;
        let blobs_dir = out
            .open_dir("blobs/sha256")
            .context("while opening blobs dir")?;

        // TODO: support multi-arch images
        let platform = PlatformBuilder::default()
            .architecture(self.target_arch.clone())
            .os("linux")
            .build()
            .context("while building platform")?;

        let mut base_deltas = Vec::new();
        let mut base_history = Vec::new();
        if let Some(base_dir) = &self.base_layers_dir {
            let manifest_path = base_dir.join("manifest.json");
            let manifest: BaseLayersManifest = serde_json::from_reader(BufReader::new(
                File::open(&manifest_path)
                    .with_context(|| format!("while opening {}", manifest_path.display()))?,
            ))
            .context("while reading base layers manifest")?;
            for mut delta in manifest.layers {
                delta.tar = base_dir.join(&delta.tar);
                delta.tar_zst = base_dir.join(&delta.tar_zst);
                base_deltas.push(delta);
            }
            base_history = manifest.history;
        }

        let mut layer_descriptors = Vec::new();
        let mut rootfs_digest_chain = Vec::new();

        for delta in base_deltas.iter().chain(self.deltas.iter()) {
            let mut tar_zst = Vec::new();
            BufReader::new(File::open(&delta.tar_zst).context("while opening tar.zst")?)
                .read_to_end(&mut tar_zst)
                .context("while reading tar zst")?;
            let tar_layer = LayerTarZst(Arc::new(tar_zst));
            let mut layer_descriptor =
                write(&blobs_dir, &tar_layer).context("while writing layer")?;
            layer_descriptor.set_platform(Some(platform.clone()));
            layer_descriptors.push(layer_descriptor);

            let mut uncompressed_tar =
                BufReader::new(File::open(&delta.tar).context("while opening uncompressed tar")?);
            let mut hasher = Sha256::new();
            std::io::copy(&mut uncompressed_tar, &mut hasher).context("while hashing tar")?;
            let layer_hash = hex::encode(hasher.finalize());
            rootfs_digest_chain.push(format!("sha256:{layer_hash}"));
        }

        let mut history = base_history;
        history.extend(self.deltas.iter().map(|delta| {
            HistoryBuilder::default()
                .created_by(delta.name.clone().unwrap_or_else(|| "antlir2".to_owned()))
                .build()
                .expect("build history entry")
        }));

        let facts_db = RoDatabase::open(&self.facts_db)
            .with_context(|| format!("while opening facts db '{}'", self.facts_db.display()))?;
        let mut labels = HashMap::new();
        for label in facts_db.iter::<OciLabel>()? {
            labels.insert(label.key.clone(), label.value.clone());
        }

        let mut env_map = HashMap::new();
        for env in facts_db.iter::<OciEnv>()? {
            env_map.insert(env.key.clone(), env.value.clone());
        }
        let env_list: Vec<String> = env_map
            .into_iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect();

        let mut user = None;
        for oci_user in facts_db.iter::<OciUser>()? {
            if let Some(existing_user) = &user {
                anyhow::bail!(
                    "duplicate OCI user '{}', already set to '{}'",
                    oci_user.user,
                    existing_user
                );
            }
            user = Some(oci_user.user.clone());
        }

        let mut cmd = None;
        for oci_cmd in facts_db.iter::<OciCmd>()? {
            if let Some(existing_cmd) = &cmd {
                anyhow::bail!(
                    "duplicate OCI cmd '{:?}', already set to '{:?}'",
                    oci_cmd.cmd,
                    existing_cmd
                );
            }
            cmd = Some(oci_cmd.cmd.clone());
        }

        let mut working_dir = None;
        for oci_working_dir in facts_db.iter::<OciWorkingDir>()? {
            if let Some(existing_working_dir) = &working_dir {
                anyhow::bail!(
                    "duplicate OCI working_dir '{}', already set to '{}'",
                    oci_working_dir.working_dir,
                    existing_working_dir
                );
            }
            working_dir = Some(oci_working_dir.working_dir.clone());
        }

        let mut stop_signal = None;
        for oci_stop_signal in facts_db.iter::<OciStopSignal>()? {
            if let Some(existing_stop_signal) = &stop_signal {
                anyhow::bail!(
                    "duplicate OCI stop_signal '{}', already set to '{}'",
                    oci_stop_signal.stop_signal,
                    existing_stop_signal
                );
            }
            stop_signal = Some(oci_stop_signal.stop_signal.clone());
        }

        let exposed_ports: BTreeSet<_> = facts_db
            .iter::<OciExposedPort>()?
            .map(|port| port.port.clone())
            .collect();

        let mut config_builder = ConfigBuilder::default()
            .entrypoint(self.entrypoint.clone())
            .labels(labels)
            .env(env_list);
        if let Some(user) = user {
            config_builder = config_builder.user(user);
        }
        if let Some(cmd) = cmd {
            config_builder = config_builder.cmd(cmd);
        }
        if let Some(working_dir) = working_dir {
            config_builder = config_builder.working_dir(working_dir);
        }
        if let Some(stop_signal) = stop_signal {
            config_builder = config_builder.stop_signal(stop_signal);
        }
        if !exposed_ports.is_empty() {
            config_builder =
                config_builder.exposed_ports(exposed_ports.into_iter().collect::<Vec<_>>());
        }

        let image_configuration = ImageConfigurationBuilder::default()
            .architecture(self.target_arch.clone())
            .os("linux")
            .created(chrono::Utc::now().to_rfc3339())
            .config(
                config_builder
                    .build()
                    .context("while building image config")?,
            )
            .rootfs(
                RootFsBuilder::default()
                    .typ("layers")
                    .diff_ids(rootfs_digest_chain)
                    .build()
                    .context("while building rootfs")?,
            )
            .history(history)
            .build()
            .context("while building image configuration")?;
        let image_config_descriptor =
            write(&blobs_dir, &image_configuration).context("while writing image configuration")?;

        let image_manifest = ImageManifestBuilder::default()
            .schema_version(2u32)
            .media_type(MediaType::ImageManifest)
            .config(image_config_descriptor)
            .layers(layer_descriptors)
            .build()
            .context("while building image manifest")?;
        let mut image_manifest_descriptor =
            write(&blobs_dir, &image_manifest).context("while writing image manifest")?;
        image_manifest_descriptor.set_annotations(Some(hashmap! {
            ANNOTATION_REF_NAME.to_owned() => self.refname.clone(),
            "built.by.exec".to_owned() => "antlir2".to_owned(),
        }));
        image_manifest_descriptor.set_platform(Some(platform));

        let index = ImageIndexBuilder::default()
            .schema_version(2u32)
            .manifests(vec![image_manifest_descriptor])
            .build()
            .context("while building index.json")?;
        index
            .to_writer_pretty(&mut BufWriter::new(
                out.create("index.json")
                    .context("while creating index.json")?,
            ))
            .context("while writing index.json")?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn oci_with_target_arch(target_arch: Arch) -> Oci {
        Oci {
            deltas: Vec::new(),
            refname: String::new(),
            skopeo: PathBuf::new(),
            target_arch,
            entrypoint: Vec::new(),
            facts_db: PathBuf::new(),
            zstd_chunked: false,
            base_layers_dir: None,
        }
    }

    #[test]
    fn accepts_goarch_target_arches() {
        assert!(
            oci_with_target_arch(Arch::Amd64)
                .validate_target_arch()
                .is_ok()
        );
        assert!(
            oci_with_target_arch(Arch::ARM64)
                .validate_target_arch()
                .is_ok()
        );
    }

    #[test]
    fn rejects_other_target_arch() {
        let oci = oci_with_target_arch(Arch::Other("x86_64".to_owned()));

        let err = oci
            .validate_target_arch()
            .expect_err("kernel architecture should be rejected");
        assert!(
            err.to_string().contains("x86_64"),
            "error should include unsupported architecture: {err:#}"
        );
    }
}
