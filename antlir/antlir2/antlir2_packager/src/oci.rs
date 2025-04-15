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
use std::str::FromStr;
use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use cap_std::fs::Dir;
use maplit::hashmap;
use oci_spec::image::ANNOTATION_REF_NAME;
use oci_spec::image::Arch;
use oci_spec::image::ConfigBuilder;
use oci_spec::image::Descriptor;
use oci_spec::image::DescriptorBuilder;
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

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Oci {
    deltas: Vec<Delta>,
    #[serde(rename = "ref")]
    refname: String,
    target_arch: Arch,
    entrypoint: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Delta {
    tar: PathBuf,
    tar_zst: PathBuf,
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

        let image_configuration = ImageConfigurationBuilder::default()
            .architecture(self.target_arch.clone())
            .os("linux")
            .config(
                ConfigBuilder::default()
                    .entrypoint(self.entrypoint.clone())
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
