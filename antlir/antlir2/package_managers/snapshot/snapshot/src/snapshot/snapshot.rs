/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::io::Write as _;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use chrono::Utc;
use clap::Parser;
use find_root::find_repo_root;
use futures::StreamExt as _;
use serde::Deserialize;
use snapshot_common::Checksums;
use tokio::process::Command;
use tracing::info;

use super::blob_status::Entry;
use super::buck_files;
use super::buck_target::DebSuite;
use super::buck_target::DescribedTarget;
use super::buck_target::YumRepo;
use super::buck_target::make_repomd_checksums;
use super::progress;
use super::storage::StorageConfig;

const BXL_TARGET: &str = "fbcode//antlir/antlir2/package_managers/snapshot:snapshot.bxl:prepare";

#[derive(Debug, Clone, Deserialize)]
struct SnapshotConfig {
    targets: Vec<TargetConfig>,
}

#[derive(Debug, Clone, Deserialize)]
struct YumArchConfig {
    arch: String,
    arch_modifier: String,
    metadata_tree: PathBuf,
    packages_baseurl: String,
    packages_indexes: Vec<PathBuf>,
}

#[derive(Debug, Clone, Deserialize)]
struct TargetConfig {
    kind: RepoKind,
    buck_package: PathBuf,
    target_name: String,
    tree_prefix: String,
    snapshot_storage: StorageConfig,
    #[serde(default)]
    package_subtargets: Vec<String>,
    // Original label of the target being snapshotted, e.g.
    // "fbcode//antlir/antlir2/package_managers/yum:kernel-6.13.2-0_fbk7"
    #[serde(default)]
    source_label: Option<String>,
    // Yum-only: grouped arch entries
    #[serde(default)]
    arches: Vec<YumArchConfig>,
    // Deb-only
    #[serde(default)]
    metadata_tree: Option<PathBuf>,
    #[serde(default)]
    packages_baseurl: Option<String>,
    #[serde(default)]
    packages_indexes: Vec<PathBuf>,
    #[serde(default)]
    architectures: Vec<String>,
    #[serde(default)]
    components: Vec<String>,
    #[serde(default)]
    distribution: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
enum RepoKind {
    Yum,
    Deb,
}

/// Run the full snapshot pipeline for one or more Buck target patterns.
///
/// Internally spawns `buck2 bxl` to analyze the targets and materialize
/// their metadata trees and package indexes, then uploads everything to
/// the per-target storage backend, and finally writes the regenerated
/// BUCK files.
#[derive(Parser, Debug)]
pub(crate) struct Snapshot {
    /// Buck target patterns to snapshot.
    #[clap(required = true)]
    targets: Vec<String>,

    /// Timestamp tag for this snapshot (e.g. "2026-05-21T15:00:00"). Used
    /// in the storage tree prefix so concurrent snapshots don't collide.
    /// Defaults to the current UTC time. ":" is allowed.
    #[clap(long)]
    timestamp: Option<String>,

    /// Path to the buck2 binary. Defaults to `buck2` in PATH.
    #[clap(long, default_value = "buck2")]
    buck2: PathBuf,
}

struct ArchResult {
    arch: String,
    arch_modifier: String,
    index_checksums: Checksums,
}

struct DebResult {
    index_checksums: Checksums,
}

impl Snapshot {
    pub(crate) async fn run(self, fb: fbinit::FacebookInit) -> Result<()> {
        // Timestamp may contain ":" (desired per user), defaults to format with ":".
        let timestamp = self
            .timestamp
            .unwrap_or_else(|| Utc::now().format("%Y-%m-%d+%H:%M:%S").to_string());
        info!(%timestamp, "snapshot timestamp");

        let project_root = {
            let current_exe = std::env::current_exe().context("while getting current_exe")?;
            find_repo_root(current_exe).context("while finding repo root")?
        };
        info!(project_root = %project_root.display(), "resolved project root");

        let bxl_pb = progress::spinner("Running buck2 bxl prepare");
        let config = run_bxl_prepare(&self.buck2, &project_root, &self.targets, &timestamp)
            .await
            .context("buck2 bxl preparation failed")?;
        bxl_pb.finish_with_message("buck2 bxl prepare complete");

        let mut described = Vec::new();

        // Compute progress total; skip targets with 0 arches (they will bail later with clear error)
        let total: usize = config
            .targets
            .iter()
            .map(|t| match t.kind {
                RepoKind::Yum => t.arches.len(),
                RepoKind::Deb => 1,
            })
            .sum();
        let overall_pb = progress::bar(total.max(1), "Snapshotting targets");

        for target in &config.targets {
            match target.kind {
                RepoKind::Yum => {
                    if target.arches.is_empty() {
                        bail!("no architectures found for target {}", target.target_name);
                    }
                    let mut arch_results = Vec::new();
                    for arch_cfg in &target.arches {
                        info!(
                            target_name = target.target_name,
                            arch = arch_cfg.arch,
                            "snapshotting yum arch"
                        );
                        let arch_pb = progress::spinner(format!(
                            "Snapshotting {} [{}]",
                            target.target_name, arch_cfg.arch
                        ));
                        let ar = snapshot_yum_arch(arch_cfg, target, &project_root, fb).await?;
                        arch_pb.finish_with_message(format!(
                            "Snapshotted {} [{}]",
                            target.target_name, arch_cfg.arch
                        ));
                        arch_results.push(ar);
                        overall_pb.inc(1);
                    }
                    let dt = generate_yum_target(target, &arch_results)?;
                    described.push(dt);
                }
                RepoKind::Deb => {
                    info!(target_name = target.target_name, "snapshotting deb suite");
                    let suite_pb =
                        progress::spinner(format!("Snapshotting {} (deb)", target.target_name));
                    let deb_result = snapshot_deb_target(target, &project_root, fb).await?;
                    suite_pb.finish_with_message(format!("Snapshotted {}", target.target_name));
                    let dt = generate_deb_target(target, &deb_result)?;
                    described.push(dt);
                    overall_pb.inc(1);
                }
            }
        }
        overall_pb.finish_with_message("All targets snapshotted");

        let rendered = buck_files::render_all(described).context("failed to render BUCK files")?;
        info!(count = rendered.len(), "writing BUCK files");
        write_buck_files_concurrent(&project_root, rendered)
            .await
            .context("failed to write BUCK files")?;

        Ok(())
    }
}

async fn write_buck_files_concurrent(
    project_root: &Path,
    rendered: BTreeMap<PathBuf, String>,
) -> Result<()> {
    // Use cap-std Dir for containment: prevents out-of-tree writes via .. or symlink escape.
    // All operations after opening the project root are relative to the Dir handle.
    let project_root_owned = project_root.to_owned();
    tokio::task::spawn_blocking(move || -> Result<()> {
        use cap_std::fs::Dir;
        let dir = Dir::open_ambient_dir(&project_root_owned, cap_std::ambient_authority())
            .with_context(|| {
                format!(
                    "failed to open project root {}",
                    project_root_owned.display()
                )
            })?;

        // Deduplicated parent creation (cap-std Dir provides containment against out-of-tree writes)
        let mut parents: BTreeSet<PathBuf> = BTreeSet::new();
        for path in rendered.keys() {
            if let Some(parent) = path.parent() {
                parents.insert(parent.to_owned());
            }
        }
        for parent in &parents {
            dir.create_dir_all(parent)
                .with_context(|| format!("failed to create directory {}", parent.display()))?;
        }

        // Sequential writes are sufficient for BUCK files (small count, small size)
        // and avoid FD exhaustion vs unbounded try_join_all. If needed, can be
        // parallelized with scoped threads bounded to 50.
        for (rel_path, content) in &rendered {
            dir.write(rel_path, content.as_bytes())
                .with_context(|| format!("failed to write {}", rel_path.display()))?;
        }

        Ok(())
    })
    .await
    .context("spawn_blocking failed")??;

    Ok(())
}

async fn run_bxl_prepare(
    buck2: &Path,
    project_root: &Path,
    targets: &[String],
    timestamp: &str,
) -> Result<SnapshotConfig> {
    let mut args: Vec<String> = vec![
        "bxl".to_owned(),
        BXL_TARGET.to_owned(),
        "--".to_owned(),
        "--timestamp".to_owned(),
        timestamp.to_owned(),
    ];
    for target in targets {
        args.push("--target".to_owned());
        args.push(target.clone());
    }

    let mut cmd = Command::new(buck2);
    cmd.current_dir(project_root);
    cmd.args(&args);
    cmd.stderr(std::process::Stdio::inherit());
    cmd.stdout(std::process::Stdio::piped());

    info!(buck2 = %buck2.display(), ?args, "spawning buck2 bxl");
    let child = cmd.spawn().context("failed to spawn buck2 bxl prepare")?;
    let output = child
        .wait_with_output()
        .await
        .context("failed to wait for buck2 bxl prepare")?;
    if !output.status.success() {
        // Include truncated stdout for debugging BXL failures
        let stdout_lossy = String::from_utf8_lossy(&output.stdout);
        let truncated = if stdout_lossy.len() > 4096 {
            let boundary = stdout_lossy.floor_char_boundary(4096);
            format!("{}... (truncated)", &stdout_lossy[..boundary])
        } else {
            stdout_lossy.to_string()
        };
        bail!(
            "buck2 bxl prepare exited with {} stdout: {}",
            output.status,
            truncated
        );
    }

    let stdout = std::str::from_utf8(&output.stdout).context("buck2 bxl stdout was not utf-8")?;
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("buck2 bxl prepare produced no stdout — config path missing. {stderr}");
    }
    let config_path = PathBuf::from(trimmed);

    let contents = tokio::fs::read_to_string(&config_path)
        .await
        .with_context(|| format!("failed to read {}", config_path.display()))?;
    serde_json::from_str(&contents)
        .with_context(|| format!("failed to parse config JSON at {}", config_path.display()))
}

/// Generic snapshot of a single metadata tree + package indexes, used for both
/// yum and deb. It builds a JSON index file that maps every file in the
/// snapshot (metadata files + packages) to its checksums, stores that index
/// in Manifold at `index.json`, and returns the checksums for the index file
/// itself. This way the generated BUCK file only needs to carry the checksum
/// for the index, and buck2 can download any other file via dynamic deps
/// using the index.
struct SnapshotOneRequest<'a> {
    metadata_tree: PathBuf,
    packages_indexes: Vec<PathBuf>,
    packages_baseurl: String,
    tree_prefix: String,
    storage_config: &'a StorageConfig,
}

async fn snapshot_one(
    req: SnapshotOneRequest<'_>,
    project_root: &Path,
    fb: fbinit::FacebookInit,
) -> Result<Checksums> {
    // Use a single storage session for the whole snapshot – Manifold provides
    // read-after-write consistency, so we don't need fallback logic and we
    // avoid creating multiple clients.
    let storage: std::sync::Arc<dyn super::storage::Storage> = req
        .storage_config
        .build_with_tree_prefix(fb, &req.tree_prefix)
        .with_context(|| format!("failed to build storage for {}", req.tree_prefix))?
        .into();

    info!(
        tree_prefix = %req.tree_prefix,
        metadata_tree = %req.metadata_tree.display(),
        "uploading metadata"
    );
    let metadata_out = super::metadata::snapshot_metadata(storage.clone(), &req.metadata_tree)
        .await
        .context("metadata snapshot failed")?;

    let all_entries = read_all_entries(project_root, &req.packages_indexes).await?;

    let mut blob_status = super::blob_status::check_blob_status(&*storage, all_entries.clone())
        .await
        .context("blob status check failed")?;

    // Move the full-checksum map out for index building before handing the rest
    // of blob_status (missing + expiring_soon) to snapshot_packages, which never
    // reads full_checksums.
    let blob_status_full = std::mem::take(&mut blob_status.full_checksums);

    let packages_out = super::packages::snapshot_packages(
        storage.clone(),
        blob_status,
        &all_entries,
        &req.packages_baseurl,
    )
    .await
    .context("packages snapshot failed")?;

    // Build a combined index of every file in the snapshot, mapping relpath
    // → checksums. For metadata we already have full checksums from the store
    // operation. For packages we reuse full checksums fetched during the blob
    // status check (flat properties) and from the packages that were newly
    // uploaded, avoiding a separate get_file_checksums RPC per file.
    let mut index: BTreeMap<String, Checksums> = BTreeMap::new();

    for (relpath, url) in &metadata_out.files {
        let cs = metadata_out.checksums.get(url).with_context(|| {
            format!(
                "metadata file '{}' (url {}) missing checksums in output – metadata upload incomplete",
                relpath, url
            )
        })?;
        index.insert(relpath.clone(), cs.clone());
    }

    // Assemble package checksums from cached data.
    let mut packages_full_map: BTreeMap<String, Checksums> = BTreeMap::new();

    // Existing blobs: full checksums were fetched during blob status check.
    for (filename, full) in &blob_status_full {
        packages_full_map.insert(filename.clone(), full.clone());
    }

    // Newly uploaded blobs: full checksums returned from store_flat.
    for (filename, url) in &packages_out.files {
        if let Some(full) = packages_out.checksums.get(url) {
            packages_full_map.insert(filename.clone(), full.clone());
        }
    }

    // Fallback for any entries not covered by cached data (e.g. race where
    // a missing blob was found to exist during upload). This path ensures
    // completeness and is expected to be rare.
    let mut fallback_entries = Vec::new();
    for entry in &all_entries {
        if !packages_full_map.contains_key(&entry.filename) {
            fallback_entries.push(entry.clone());
        }
    }

    if !fallback_entries.is_empty() {
        info!(
            count = fallback_entries.len(),
            "fetching full checksums for {} fallback entries",
            fallback_entries.len()
        );
        let checksum_pb = progress::bar(
            fallback_entries.len(),
            "Fetching fallback package checksums",
        );
        // Retry each fetch with exponential backoff, at lower concurrency.
        let fallback_checksums = futures::stream::iter(fallback_entries.into_iter().map(|entry| {
            let relpath = entry.filename.clone();
            let storage = storage.clone();
            let pb = checksum_pb.clone();
            async move {
                let full = backoff::future::retry(super::storage::retry_policy(), || async {
                    storage
                        .get_file_checksums(&relpath)
                        .await
                        .map_err(backoff::Error::transient)
                })
                .await
                .with_context(|| format!("failed to get checksums for {}", relpath))?;
                pb.inc(1);
                Ok::<_, anyhow::Error>((relpath, full))
            }
        }))
        .buffer_unordered(50)
        .collect::<Vec<Result<_, _>>>()
        .await
        .into_iter()
        .collect::<Result<Vec<_>, _>>()
        .context("failed to fetch fallback package checksums")?;
        checksum_pb.finish_with_message("Fetched fallback checksums");

        for (relpath, cs) in fallback_checksums {
            packages_full_map.insert(relpath, cs);
        }
    }

    for (relpath, cs) in packages_full_map {
        index.insert(relpath, cs);
    }

    // Serialize index to a temp file and store it at index.json.
    let index_pb = progress::spinner("Writing index.json");
    let tmp = tokio::task::spawn_blocking(move || -> Result<tempfile::NamedTempFile> {
        let mut tmp =
            tempfile::NamedTempFile::new().context("failed to create temp file for index")?;
        serde_json::to_writer(&mut tmp, &index).context("failed to write index JSON")?;
        tmp.flush().context("failed to flush index JSON")?;
        Ok(tmp)
    })
    .await
    .context("spawn_blocking failed")??;

    let stored = storage
        .store(tmp.path(), "index.json")
        .await
        .context("failed to store index.json")?;
    index_pb.finish_with_message("Wrote index.json");

    Ok(stored.checksums)
}

async fn snapshot_yum_arch(
    arch: &YumArchConfig,
    target: &TargetConfig,
    project_root: &Path,
    fb: fbinit::FacebookInit,
) -> Result<ArchResult> {
    let tree_prefix = format!("{}/{}", target.tree_prefix, arch.arch);
    let metadata_tree = project_root.join(&arch.metadata_tree);

    let index_checksums = snapshot_one(
        SnapshotOneRequest {
            metadata_tree,
            packages_indexes: arch.packages_indexes.clone(),
            packages_baseurl: arch.packages_baseurl.clone(),
            tree_prefix,
            storage_config: &target.snapshot_storage,
        },
        project_root,
        fb,
    )
    .await?;

    Ok(ArchResult {
        arch: arch.arch.clone(),
        arch_modifier: arch.arch_modifier.clone(),
        index_checksums,
    })
}

async fn snapshot_deb_target(
    target: &TargetConfig,
    project_root: &Path,
    fb: fbinit::FacebookInit,
) -> Result<DebResult> {
    let metadata_tree = project_root.join(
        target
            .metadata_tree
            .as_ref()
            .context("deb target missing metadata_tree")?,
    );

    let baseurl = target
        .packages_baseurl
        .as_deref()
        .context("deb target missing packages_baseurl")?
        .to_owned();

    let index_checksums = snapshot_one(
        SnapshotOneRequest {
            metadata_tree,
            packages_indexes: target.packages_indexes.clone(),
            packages_baseurl: baseurl,
            tree_prefix: target.tree_prefix.clone(),
            storage_config: &target.snapshot_storage,
        },
        project_root,
        fb,
    )
    .await?;

    Ok(DebResult { index_checksums })
}

async fn read_all_entries(project_root: &Path, paths: &[PathBuf]) -> Result<Vec<Entry>> {
    // Use cap-std Dir to contain reads to project_root
    let project_root_owned = project_root.to_owned();
    let paths_owned = paths.to_owned();
    let per_index = tokio::task::spawn_blocking(move || -> Result<Vec<Vec<Entry>>> {
        use cap_std::fs::Dir;
        let dir = Dir::open_ambient_dir(&project_root_owned, cap_std::ambient_authority())
            .with_context(|| {
                format!(
                    "failed to open project root {}",
                    project_root_owned.display()
                )
            })?;
        let mut results = Vec::new();
        for rel in &paths_owned {
            let contents = dir
                .read_to_string(rel)
                .with_context(|| format!("failed to read {}", rel.display()))?;
            let parsed: Vec<Entry> = serde_json::from_str(&contents)
                .with_context(|| format!("failed to parse {}", rel.display()))?;
            results.push(parsed);
        }
        Ok(results)
    })
    .await
    .context("spawn_blocking failed")??;

    Ok(per_index.into_iter().flatten().collect())
}

fn common_parts(
    target: &TargetConfig,
) -> Result<(
    BTreeMap<String, String>,
    BTreeSet<String>,
    Option<buck_label::Label>,
)> {
    let storage_map = target
        .snapshot_storage
        .as_user_dict()
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v))
        .collect();
    let package_subtargets = target
        .package_subtargets
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    let snapshot_source = match &target.source_label {
        Some(s) => Some(
            buck_label::Label::new(s.clone())
                .with_context(|| format!("invalid source_label: {}", s))?,
        ),
        None => None,
    };
    Ok((storage_map, package_subtargets, snapshot_source))
}

fn generate_yum_target(
    target: &TargetConfig,
    arch_results: &[ArchResult],
) -> Result<DescribedTarget> {
    if arch_results.is_empty() {
        bail!("no architectures found for target {}", target.target_name);
    }

    let tree_base_url = target
        .snapshot_storage
        .tree_base_url_with_prefix(&target.tree_prefix);
    let baseurl = format!("{}{{arch}}/", tree_base_url);

    let arch_names: Vec<String> = arch_results.iter().map(|r| r.arch.clone()).collect();

    let entries: Vec<(&str, &str, &Checksums)> = arch_results
        .iter()
        .map(|r| {
            (
                r.arch.as_str(),
                r.arch_modifier.as_str(),
                &r.index_checksums,
            )
        })
        .collect();
    let index_checksums = make_repomd_checksums(&entries)?;

    let (storage_map, package_subtargets, snapshot_source) = common_parts(target)?;

    let repo = YumRepo {
        name: target.target_name.clone(),
        arches: arch_names,
        baseurl,
        package_subtargets,
        index_checksums,
        snapshot_source,
        snapshot_storage: storage_map,
        visibility: vec!["PUBLIC".to_owned()],
    };

    Ok(DescribedTarget {
        package_path: target.buck_package.clone(),
        target: Box::new(repo),
    })
}

fn generate_deb_target(target: &TargetConfig, result: &DebResult) -> Result<DescribedTarget> {
    let tree_base_url = target
        .snapshot_storage
        .tree_base_url_with_prefix(&target.tree_prefix);
    let archive_url = tree_base_url.clone();

    let distribution = target
        .distribution
        .as_deref()
        .context("deb target missing distribution")?
        .to_owned();

    let (storage_map, package_subtargets, snapshot_source) = common_parts(target)?;

    let suite = DebSuite {
        name: target.target_name.clone(),
        architectures: target.architectures.clone(),
        archive_url,
        components: target.components.clone(),
        distribution,
        index_checksums: result.index_checksums.clone(),
        package_subtargets,
        snapshot_source,
        snapshot_storage: storage_map,
        visibility: vec!["PUBLIC".to_owned()],
    };

    Ok(DescribedTarget {
        package_path: target.buck_package.clone(),
        target: Box::new(suite),
    })
}
