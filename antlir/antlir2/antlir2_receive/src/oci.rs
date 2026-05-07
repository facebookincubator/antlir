/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashMap;
use std::fs;
use std::fs::File;
use std::io;
use std::io::BufReader;
use std::io::ErrorKind;
use std::path::Component;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use flate2::read::GzDecoder;
use oci_spec::image::ANNOTATION_REF_NAME;
use oci_spec::image::Descriptor;
use oci_spec::image::History;
use oci_spec::image::ImageConfiguration;
use oci_spec::image::ImageIndex;
use oci_spec::image::ImageManifest;
use oci_spec::image::MediaType;
use serde::Serialize;
use tempfile::NamedTempFile;

enum LayerTar {
    Source(PathBuf),
    Temp(NamedTempFile),
}

#[derive(Debug, PartialEq, Eq)]
enum Whiteout {
    Opaque(PathBuf),
    Remove(PathBuf),
    Special,
}

impl LayerTar {
    fn path(&self) -> &Path {
        match self {
            Self::Source(path) => path,
            Self::Temp(file) => file.path(),
        }
    }
}

pub(crate) fn recv_oci(
    source: &Path,
    dst: &Path,
    oci_ref: Option<&str>,
    oci_arch: Option<&str>,
) -> Result<()> {
    let manifest = load_manifest(source, oci_ref, oci_arch)?;

    for descriptor in manifest.layers() {
        let layer = materialize_layer_tar(source, descriptor)
            .with_context(|| format!("while materializing layer {}", descriptor.digest()))?;
        apply_layer(layer.path(), dst)
            .with_context(|| format!("while applying layer {}", descriptor.digest()))?;
    }

    Ok(())
}

pub(crate) struct OciMetadata {
    pub labels: HashMap<String, String>,
    pub env: Vec<String>,
}

#[derive(Serialize)]
struct LayerEntry {
    tar: String,
    tar_zst: String,
}

#[derive(Serialize)]
struct LayersManifest {
    layers: Vec<LayerEntry>,
    history: Vec<History>,
}

pub(crate) fn extract_oci_layers(
    source: &Path,
    oci_layers_out: &Path,
    oci_ref: Option<&str>,
    oci_arch: Option<&str>,
) -> Result<OciMetadata> {
    std::fs::create_dir_all(oci_layers_out)
        .context("while creating OCI layers output directory")?;

    let manifest = load_manifest(source, oci_ref, oci_arch)?;
    let config = ImageConfiguration::from_file(blob_path(source, manifest.config())?)
        .with_context(|| format!("while reading config {}", manifest.config().digest()))?;

    let mut layer_entries = Vec::new();
    for (i, layer_descriptor) in manifest.layers().iter().enumerate() {
        let tar_name = format!("{i}.tar");
        let tar_zst_name = format!("{i}.tar.zst");
        let tar_path = oci_layers_out.join(&tar_name);
        let tar_zst_path = oci_layers_out.join(&tar_zst_name);

        match layer_descriptor.media_type() {
            MediaType::ImageLayerZstd | MediaType::ImageLayerNonDistributableZstd => {
                let blob = blob_path(source, layer_descriptor)?;
                std::fs::copy(&blob, &tar_zst_path)
                    .with_context(|| format!("while copying zstd layer {i} blob"))?;
                let mut decoder = zstd::Decoder::new(BufReader::new(
                    File::open(&tar_zst_path)
                        .with_context(|| format!("while opening layer {i} for decompression"))?,
                ))
                .with_context(|| format!("while creating zstd decoder for layer {i}"))?;
                let mut tar_out = File::create(&tar_path)
                    .with_context(|| format!("while creating layer {i} tar"))?;
                io::copy(&mut decoder, &mut tar_out)
                    .with_context(|| format!("while decompressing layer {i}"))?;
            }
            _ => {
                let layer_tar = materialize_layer_tar(source, layer_descriptor)
                    .with_context(|| format!("while materializing layer {i}"))?;
                match layer_tar {
                    LayerTar::Source(src) => {
                        std::fs::copy(&src, &tar_path)
                            .with_context(|| format!("while copying layer {i} tar"))?;
                    }
                    LayerTar::Temp(tmp) => {
                        tmp.persist(&tar_path)
                            .with_context(|| format!("while persisting layer {i} tar"))?;
                    }
                }
                let input = File::open(&tar_path)
                    .with_context(|| format!("while opening layer {i} tar for compression"))?;
                let output = File::create(&tar_zst_path)
                    .with_context(|| format!("while creating layer {i} tar.zst"))?;
                zstd::stream::copy_encode(BufReader::new(input), output, 15)
                    .with_context(|| format!("while compressing layer {i} to zstd"))?;
            }
        }

        layer_entries.push(LayerEntry {
            tar: tar_name,
            tar_zst: tar_zst_name,
        });
    }

    let layers_manifest = LayersManifest {
        layers: layer_entries,
        history: config.history().clone(),
    };
    serde_json::to_writer_pretty(
        File::create(oci_layers_out.join("manifest.json"))
            .context("while creating layers manifest")?,
        &layers_manifest,
    )
    .context("while writing layers manifest")?;

    let container_config = config.config().clone().unwrap_or_default();
    Ok(OciMetadata {
        labels: container_config.labels().clone().unwrap_or_default(),
        env: container_config.env().clone().unwrap_or_default(),
    })
}

fn load_manifest(
    source: &Path,
    ref_name: Option<&str>,
    arch: Option<&str>,
) -> Result<ImageManifest> {
    let index = ImageIndex::from_file(source.join("index.json"))
        .with_context(|| format!("while reading OCI index from '{}'", source.display()))?;
    let descriptor = select_manifest(index.manifests(), ref_name, arch)?;
    ImageManifest::from_file(blob_path(source, descriptor)?)
        .with_context(|| format!("while reading manifest {}", descriptor.digest()))
}

fn select_manifest<'a>(
    manifests: &'a [Descriptor],
    ref_name: Option<&str>,
    arch: Option<&str>,
) -> Result<&'a Descriptor> {
    let candidates: Vec<&Descriptor> = manifests
        .iter()
        .filter(|descriptor| {
            ref_name.is_none_or(|ref_name| {
                descriptor
                    .annotations()
                    .as_ref()
                    .and_then(|annotations| annotations.get(ANNOTATION_REF_NAME))
                    .is_some_and(|candidate| candidate == ref_name)
            })
        })
        .collect();

    if candidates.is_empty() {
        bail!(
            "OCI layout does not contain requested ref '{}'",
            ref_name.unwrap_or("<any>")
        );
    }

    let Some(arch) = arch else {
        return candidates
            .first()
            .copied()
            .context("OCI layout does not contain any manifests");
    };

    if let Some(descriptor) = candidates.iter().find(|descriptor| {
        descriptor.platform().as_ref().is_some_and(|platform| {
            platform.architecture().to_string() == arch && platform.os().to_string() == "linux"
        })
    }) {
        return Ok(descriptor);
    }

    // Single-arch images may omit platform metadata entirely — accept
    // only if there is exactly one candidate and it has no platform set
    if candidates.len() == 1 && candidates[0].platform().is_none() {
        return Ok(candidates[0]);
    }

    bail!(
        "OCI layout does not contain requested platform linux/{arch} for ref '{}'",
        ref_name.unwrap_or("<any>")
    )
}

fn blob_path(source: &Path, descriptor: &Descriptor) -> Result<PathBuf> {
    let digest = descriptor
        .as_digest_sha256()
        .with_context(|| format!("unsupported non-sha256 digest {}", descriptor.digest()))?;
    Ok(source.join("blobs").join("sha256").join(digest))
}

const DOCKER_V2_LAYER_GZIP: &str = "application/vnd.docker.image.rootfs.diff.tar.gzip";

fn is_gzip_media_type(media_type: &MediaType) -> bool {
    match media_type {
        MediaType::ImageLayerGzip | MediaType::ImageLayerNonDistributableGzip => true,
        MediaType::Other(s) if s == DOCKER_V2_LAYER_GZIP => true,
        _ => false,
    }
}

fn materialize_layer_tar(source: &Path, descriptor: &Descriptor) -> Result<LayerTar> {
    let path = blob_path(source, descriptor)?;
    let open = || File::open(&path).with_context(|| format!("while opening {}", path.display()));

    let media_type = descriptor.media_type();
    match media_type {
        MediaType::ImageLayer | MediaType::ImageLayerNonDistributable => Ok(LayerTar::Source(path)),
        _ if is_gzip_media_type(media_type) => {
            let mut temp = NamedTempFile::new().context("while creating temporary layer tar")?;
            let mut decoder = GzDecoder::new(BufReader::new(open()?));
            io::copy(&mut decoder, temp.as_file_mut())
                .with_context(|| format!("while decompressing gzip layer {}", path.display()))?;
            Ok(LayerTar::Temp(temp))
        }
        MediaType::ImageLayerZstd | MediaType::ImageLayerNonDistributableZstd => {
            let mut temp = NamedTempFile::new().context("while creating temporary layer tar")?;
            let mut decoder = zstd::Decoder::new(BufReader::new(open()?))
                .with_context(|| format!("while creating zstd decoder for {}", path.display()))?;
            io::copy(&mut decoder, temp.as_file_mut())
                .with_context(|| format!("while decompressing zstd layer {}", path.display()))?;
            Ok(LayerTar::Temp(temp))
        }
        other => {
            bail!("unsupported OCI layer media type '{other}'")
        }
    }
}

fn apply_layer(layer_tar: &Path, dst: &Path) -> Result<()> {
    let whiteouts = scan_whiteouts(layer_tar)?;
    for whiteout in whiteouts {
        apply_whiteout(dst, whiteout)?;
    }

    extract_layer_tar(layer_tar, dst)
}

fn extract_layer_tar(layer_tar: &Path, dst: &Path) -> Result<()> {
    let status = Command::new("tar")
        .arg("--extract")
        .arg("--file")
        .arg(layer_tar)
        .arg("--directory")
        .arg(dst)
        .arg("--numeric-owner")
        .arg("--same-owner")
        .arg("--preserve-permissions")
        .arg("--xattrs")
        .arg("--acls")
        .arg("--selinux")
        .arg("--delay-directory-restore")
        .arg("--exclude=.wh.*")
        .arg("--exclude=*/.wh.*")
        .status()
        .with_context(|| format!("while extracting {}", layer_tar.display()))?;
    if !status.success() {
        bail!(
            "tar failed while extracting {} with status {status}",
            layer_tar.display()
        );
    }
    Ok(())
}

fn scan_whiteouts(layer_tar: &Path) -> Result<Vec<Whiteout>> {
    let mut archive = tar::Archive::new(BufReader::new(
        File::open(layer_tar).with_context(|| format!("while opening {}", layer_tar.display()))?,
    ));

    archive
        .entries()
        .context("while reading layer tar")?
        .map(|entry| {
            let entry = entry.context("while reading layer tar entry")?;
            let path = entry
                .path()
                .context("while reading layer tar entry path")?
                .into_owned();
            validate_relative_path(&path)?;
            parse_whiteout(&path)
        })
        .filter_map(Result::transpose)
        .collect()
}

fn parse_whiteout(path: &Path) -> Result<Option<Whiteout>> {
    let Some(file_name) = path.file_name().and_then(|file_name| file_name.to_str()) else {
        return Ok(None);
    };
    let parent = path.parent().unwrap_or_else(|| Path::new(""));

    if file_name == ".wh..wh..opq" {
        return Ok(Some(Whiteout::Opaque(normalize_relative_path(parent)?)));
    }

    if file_name.starts_with(".wh..wh.") {
        return Ok(Some(Whiteout::Special));
    }

    let Some(removed) = file_name.strip_prefix(".wh.") else {
        return Ok(None);
    };
    Ok(Some(Whiteout::Remove(normalize_whiteout_target(
        &parent.join(removed),
    )?)))
}

fn apply_whiteout(dst: &Path, whiteout: Whiteout) -> Result<()> {
    match whiteout {
        Whiteout::Opaque(path) => remove_children(dst, &path),
        Whiteout::Remove(path) => remove_path(dst, &path),
        Whiteout::Special => Ok(()),
    }
}

fn remove_children(root: &Path, path: &Path) -> Result<()> {
    let Some(path) = checked_child_path(root, path)? else {
        return Ok(());
    };
    let metadata = match path.symlink_metadata() {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(error).with_context(|| format!("while reading {}", path.display()));
        }
    };
    if metadata.file_type().is_symlink() {
        bail!("opaque OCI whiteout path '{}' is a symlink", path.display());
    }
    if !metadata.is_dir() {
        return Ok(());
    }

    for entry in fs::read_dir(&path).with_context(|| format!("while reading {}", path.display()))? {
        let entry = entry.with_context(|| format!("while reading child of {}", path.display()))?;
        remove_path(&path, &PathBuf::from(entry.file_name()))?;
    }
    Ok(())
}

fn remove_path(root: &Path, path: &Path) -> Result<()> {
    let Some(path) = checked_child_path(root, path)? else {
        return Ok(());
    };
    let metadata = match path.symlink_metadata() {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(error).with_context(|| format!("while reading {}", path.display()));
        }
    };

    if metadata.is_dir() {
        fs::remove_dir_all(&path).with_context(|| format!("while removing {}", path.display()))
    } else {
        fs::remove_file(&path).with_context(|| format!("while removing {}", path.display()))
    }
}

fn checked_child_path(root: &Path, path: &Path) -> Result<Option<PathBuf>> {
    let mut current = root.to_owned();
    let mut components = path.components().peekable();
    while let Some(component) = components.next() {
        let Component::Normal(name) = component else {
            bail!("OCI whiteout path '{}' is not normalized", path.display());
        };
        current.push(name);

        if components.peek().is_none() {
            return Ok(Some(current));
        }

        let metadata = match current.symlink_metadata() {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(error).with_context(|| format!("while reading {}", current.display()));
            }
        };
        if metadata.file_type().is_symlink() {
            bail!(
                "OCI whiteout path '{}' crosses symlink '{}'",
                path.display(),
                current.display()
            );
        }
        if !metadata.is_dir() {
            return Ok(None);
        }
    }
    Ok(Some(current))
}

fn validate_relative_path(path: &Path) -> Result<()> {
    for component in path.components() {
        match component {
            Component::CurDir | Component::Normal(_) => {}
            Component::ParentDir | Component::Prefix(_) | Component::RootDir => {
                bail!("OCI layer path '{}' is not relative", path.display())
            }
        }
    }
    Ok(())
}

fn normalize_whiteout_target(path: &Path) -> Result<PathBuf> {
    let normalized = normalize_relative_path(path)?;
    if normalized.as_os_str().is_empty() {
        bail!("OCI whiteout target '{}' is empty", path.display());
    }
    Ok(normalized)
}

fn normalize_relative_path(path: &Path) -> Result<PathBuf> {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(component) => normalized.push(component),
            Component::ParentDir | Component::Prefix(_) | Component::RootDir => {
                bail!("OCI whiteout path '{}' is not relative", path.display())
            }
        }
    }
    Ok(normalized)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::fs;
    use std::os::unix::fs::symlink;
    use std::str::FromStr;

    use oci_spec::image::Arch;
    use oci_spec::image::DescriptorBuilder;
    use oci_spec::image::PlatformBuilder;
    use oci_spec::image::Sha256Digest;
    use tempfile::tempdir;

    use super::*;

    fn descriptor(ref_name: &str, arch: Arch) -> Descriptor {
        DescriptorBuilder::default()
            .media_type(MediaType::ImageManifest)
            .digest(
                Sha256Digest::from_str(
                    "0000000000000000000000000000000000000000000000000000000000000000",
                )
                .expect("valid digest"),
            )
            .size(0_u64)
            .annotations(HashMap::from([(
                ANNOTATION_REF_NAME.to_owned(),
                ref_name.to_owned(),
            )]))
            .platform(
                PlatformBuilder::default()
                    .architecture(arch)
                    .os("linux")
                    .build()
                    .expect("valid platform"),
            )
            .build()
            .expect("valid descriptor")
    }

    #[test]
    fn rejects_multi_manifest_arch_mismatch() {
        let manifests = vec![
            descriptor("latest", Arch::Amd64),
            descriptor("latest", Arch::ARM64),
        ];

        let error = select_manifest(&manifests, Some("latest"), Some("s390x"))
            .expect_err("no matching arch among multiple manifests should fail");

        assert!(
            error.to_string().contains("linux/s390x"),
            "error should mention requested platform: {error:#}"
        );
    }

    #[test]
    fn selects_requested_arch_match() {
        let manifests = vec![descriptor("latest", Arch::Amd64)];

        let descriptor = select_manifest(&manifests, Some("latest"), Some("amd64"))
            .expect("requested arch should match");

        assert_eq!(
            descriptor
                .platform()
                .as_ref()
                .expect("descriptor should have platform")
                .architecture()
                .to_string(),
            "amd64"
        );
    }

    fn descriptor_no_platform(ref_name: &str) -> Descriptor {
        DescriptorBuilder::default()
            .media_type(MediaType::ImageManifest)
            .digest(
                Sha256Digest::from_str(
                    "0000000000000000000000000000000000000000000000000000000000000000",
                )
                .expect("valid digest"),
            )
            .size(0_u64)
            .annotations(HashMap::from([(
                ANNOTATION_REF_NAME.to_owned(),
                ref_name.to_owned(),
            )]))
            .build()
            .expect("valid descriptor")
    }

    #[test]
    fn falls_back_to_single_manifest_without_platform() {
        let manifests = vec![descriptor_no_platform("latest")];

        let result = select_manifest(&manifests, Some("latest"), Some("amd64"));
        assert!(
            result.is_ok(),
            "single manifest without platform should be accepted"
        );
    }

    #[test]
    fn rejects_single_manifest_with_wrong_platform() {
        let manifests = vec![descriptor("latest", Arch::Amd64)];

        let error = select_manifest(&manifests, Some("latest"), Some("arm64"))
            .expect_err("single manifest with explicit wrong platform should fail");
        assert!(
            error.to_string().contains("linux/arm64"),
            "error should mention requested platform: {error:#}"
        );
    }

    #[test]
    fn rejects_empty_or_dot_whiteout_targets() {
        for path in [".wh.", ".wh..", ".wh..."] {
            let error = parse_whiteout(Path::new(path)).expect_err("invalid target should fail");
            assert!(
                error.to_string().contains("OCI whiteout"),
                "error should explain invalid whiteout target for {path}: {error:#}"
            );
        }
    }

    #[test]
    fn parses_valid_whiteouts() {
        assert!(matches!(
            parse_whiteout(Path::new("dir/.wh.removed")).expect("valid remove whiteout"),
            Some(Whiteout::Remove(path)) if path == Path::new("dir/removed")
        ));
        assert!(matches!(
            parse_whiteout(Path::new("dir/.wh..wh..opq")).expect("valid opaque whiteout"),
            Some(Whiteout::Opaque(path)) if path == Path::new("dir")
        ));
        assert!(
            parse_whiteout(Path::new("dir/file"))
                .expect("non-whiteout path should parse")
                .is_none()
        );
    }

    #[test]
    fn whiteout_rejects_symlink_intermediate() {
        let temp = tempdir().expect("create tempdir");
        let root = temp.path().join("root");
        let outside = temp.path().join("outside");
        fs::create_dir_all(&root).expect("create root");
        fs::create_dir_all(&outside).expect("create outside");
        fs::write(outside.join("victim"), b"outside").expect("write outside victim");
        symlink("../outside", root.join("link")).expect("create symlink");

        let error = apply_whiteout(&root, Whiteout::Remove(PathBuf::from("link/victim")))
            .expect_err("whiteout should reject symlink intermediates");

        assert!(
            error.to_string().contains("crosses symlink"),
            "error should mention symlink traversal: {error:#}"
        );
        assert!(
            outside.join("victim").exists(),
            "whiteout must not delete outside root"
        );
    }

    #[test]
    fn whiteout_removes_final_symlink_without_following() {
        let temp = tempdir().expect("create tempdir");
        let root = temp.path().join("root");
        let outside = temp.path().join("outside");
        fs::create_dir_all(&root).expect("create root");
        fs::create_dir_all(&outside).expect("create outside");
        fs::write(outside.join("victim"), b"outside").expect("write outside victim");
        symlink("../outside/victim", root.join("link")).expect("create symlink");

        apply_whiteout(&root, Whiteout::Remove(PathBuf::from("link")))
            .expect("whiteout should remove the symlink itself");

        assert!(!root.join("link").exists(), "symlink should be removed");
        assert!(
            outside.join("victim").exists(),
            "whiteout must not delete symlink target"
        );
    }
}
