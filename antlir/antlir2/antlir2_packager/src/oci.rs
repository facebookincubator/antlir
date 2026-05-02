/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

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
use tempfile::TempDir;
use tempfile::tempdir;

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

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Oci {
    deltas: Vec<Delta>,
    #[serde(rename = "ref")]
    refname: String,
    skopeo: PathBuf,
    skopeo_policy: PathBuf,
    target_arch: Arch,
    entrypoint: Vec<String>,
    facts_db: PathBuf,
    #[serde(default)]
    zstd_chunked: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Delta {
    tar: PathBuf,
    tar_zst: PathBuf,
    #[serde(default)]
    name: Option<String>,
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
        let input_oci = tempdir().context("while creating temporary OCI directory")?;
        self.build_layout(input_oci.path())?;

        std::fs::create_dir_all(out).context("while creating output directory")?;
        let skopeo_dir = self.extract_skopeo_bundle()?;
        let skopeo_root = skopeo_dir.path().join("skopeo_bundle");
        let skopeo_binary = skopeo_root.join("skopeo");

        let mut cmd = self.skopeo_copy_command(&skopeo_binary, &skopeo_root, input_oci.path(), out);
        run_cmd(&mut cmd).context("while converting OCI layout to zstd:chunked")?;

        Ok(())
    }

    fn skopeo_copy_command(
        &self,
        skopeo_binary: &Path,
        skopeo_root: &Path,
        input_oci: &Path,
        out: &Path,
    ) -> Command {
        let src_oci_arg = format!("oci:{}:{}", input_oci.display(), self.refname);
        let dest_oci_arg = format!("oci:{}:{}", out.display(), self.refname);

        let mut cmd = Command::new(skopeo_binary);
        cmd.env("LD_LIBRARY_PATH", skopeo_root)
            .arg("--policy")
            .arg(&self.skopeo_policy)
            .arg("copy")
            .arg("--dest-compress-format=zstd:chunked")
            .arg(src_oci_arg)
            .arg(dest_oci_arg);
        cmd
    }

    fn extract_skopeo_bundle(&self) -> Result<TempDir> {
        let extracted = tempdir().context("while creating temporary skopeo directory")?;
        run_cmd(
            Command::new("tar")
                .arg("-xf")
                .arg(&self.skopeo)
                .arg("-C")
                .arg(extracted.path()),
        )
        .context("while extracting skopeo bundle")?;
        Ok(extracted)
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

        let mut layer_descriptors = Vec::new();
        let mut rootfs_digest_chain = Vec::new();
        for delta in &self.deltas {
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

        let history: Vec<_> = self
            .deltas
            .iter()
            .map(|delta| {
                HistoryBuilder::default()
                    .created_by(delta.name.clone().unwrap_or_else(|| "antlir2".to_owned()))
                    .build()
                    .expect("build history entry")
            })
            .collect();

        let facts_db = RoDatabase::open(&self.facts_db)
            .with_context(|| format!("while opening facts db '{}'", self.facts_db.display()))?;
        let mut labels = HashMap::new();
        for label in facts_db.iter::<OciLabel>()? {
            if labels.contains_key(&label.key) {
                anyhow::bail!(
                    "duplicate label '{}', already set to '{}'",
                    label.key,
                    labels[&label.key]
                );
            }
            labels.insert(label.key.clone(), label.value.clone());
        }

        let mut env_map = HashMap::new();
        for env in facts_db.iter::<OciEnv>()? {
            if env_map.contains_key(&env.key) {
                anyhow::bail!(
                    "duplicate env '{}', already set to '{}'",
                    env.key,
                    env_map[&env.key]
                );
            }
            env_map.insert(env.key.clone(), env.value.clone());
        }
        let env_list: Vec<String> = env_map
            .into_iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect();

        let image_configuration = ImageConfigurationBuilder::default()
            .architecture(self.target_arch.clone())
            .os("linux")
            .created(chrono::Utc::now().to_rfc3339())
            .config(
                ConfigBuilder::default()
                    .entrypoint(self.entrypoint.clone())
                    .labels(labels)
                    .env(env_list)
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
            skopeo_policy: PathBuf::new(),
            target_arch,
            entrypoint: Vec::new(),
            facts_db: PathBuf::new(),
            zstd_chunked: false,
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
