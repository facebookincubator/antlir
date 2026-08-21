/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeMap;
use std::io::BufWriter;
use std::io::Write as _;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use backoff::ExponentialBackoff;
use backoff::ExponentialBackoffBuilder;
use backoff::backoff::Backoff as _;
use clap::Parser;
use futures::stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use json_arg::Json;
use json_arg::JsonFile;
use reqwest::StatusCode;
use reqwest::header::RANGE;
use tempfile::NamedTempFile;
use tokio::io::AsyncSeekExt as _;
use tokio::io::AsyncWriteExt as _;
use tracing::debug;
use tracing::error;
use tracing::info;
use tracing::warn;

use super::Out;
use super::blob_status::BlobStatusOutput;
use super::blob_status::Entry;
use super::progress;
use super::storage::BlobStatus;
use super::storage::MAX_CONCURRENT_SYMLINK;
use super::storage::Storage;
use super::storage::StorageConfig;
use super::storage::UrlWithChecksums;

/// Everything tunable about fetching packages from an upstream mirror and
/// pushing them into storage.
///
/// These knobs only make sense relative to each other — the stall timeout, the
/// throughput floor and the attempt count together decide how patient the
/// downloader is — so they live in one place rather than as scattered
/// constants.
#[derive(Debug, Clone, Copy)]
struct PackageDownloadSettings {
    /// How many packages to download and upload at once.
    concurrency: usize,
    /// Concurrency for the second pass over packages that failed the first one.
    /// Deliberately tiny: a mirror starving us of bandwidth is often doing so
    /// *because* of the connections the first pass opened against it, so the
    /// stragglers get a much larger share of the pipe on their own.
    straggler_concurrency: usize,
    /// Give up on the straggler pass entirely past this many failures. Running
    /// four-wide only makes sense for a handful of stragglers; if a large
    /// fraction of the repo failed, the mirror is down rather than congested
    /// and a narrow replay of every entry would take longer than the run that
    /// produced them.
    max_stragglers: usize,
    connect_timeout: Duration,
    /// How long a transfer may make *zero* progress before the connection is
    /// declared dead. reqwest resets this on every successful read, so an
    /// arbitrarily slow but still-moving download is never killed by it.
    read_stall_timeout: Duration,
    /// Throughput floor used to turn a response's `Content-Length` into a
    /// deadline. Only a mirror performing worse than this is treated as broken
    /// rather than merely slow — at this rate the 1.3 GiB `0ad-data` package
    /// still gets ~90 minutes, roughly 7x what it needs at 1.7 MB/s.
    min_bytes_per_sec: u64,
    /// Floor on the derived deadline, so small packages still get a grace
    /// period larger than a single TCP hiccup.
    min_body_deadline: Duration,
    /// Ceiling on the derived deadline, and the deadline used when the server
    /// sends no `Content-Length` to scale by.
    max_body_deadline: Duration,
    /// Attempts per package before giving up. Attempts resume from the partial
    /// file, so this is N chances to make forward progress rather than N
    /// downloads from scratch.
    download_attempts: usize,
    upload_attempts: usize,
    /// Transfers slower than this get logged, so a persistently bad mirror is
    /// visible without having to reproduce the run.
    slow_transfer_warn: Duration,
    /// Responses at least this large get their own byte-level progress bar.
    /// Without one a multi-gigabyte package is indistinguishable from a hang.
    byte_progress_min_bytes: u64,
}

impl Default for PackageDownloadSettings {
    fn default() -> Self {
        Self {
            concurrency: 100,
            straggler_concurrency: 4,
            max_stragglers: 50,
            connect_timeout: Duration::from_secs(30),
            read_stall_timeout: Duration::from_secs(60),
            min_bytes_per_sec: 256 * 1024,
            min_body_deadline: Duration::from_secs(120),
            max_body_deadline: Duration::from_secs(4 * 60 * 60),
            download_attempts: 5,
            upload_attempts: 5,
            slow_transfer_warn: Duration::from_secs(30),
            byte_progress_min_bytes: 64 * 1024 * 1024,
        }
    }
}

impl PackageDownloadSettings {
    fn http_client(&self) -> Result<reqwest::Client> {
        reqwest::Client::builder()
            // Deliberately no total request timeout. A legitimate multi-gigabyte
            // package on a slow mirror runs for the better part of an hour, and
            // any fixed value is either too short for those or useless for
            // everything else. Liveness comes from `read_timeout` (which resets
            // on every successful read) plus the Content-Length-derived deadline
            // applied in `download_into`.
            .read_timeout(self.read_stall_timeout)
            .connect_timeout(self.connect_timeout)
            // reqwest negotiates content encodings by default. Package archives
            // are already compressed, so that buys nothing and costs a lot: a
            // transfer-encoded response has no usable Content-Length (so no
            // size-derived deadline) and its byte offsets are offsets into the
            // *encoded* stream, which would make `Range` resume produce garbage.
            .no_gzip()
            .no_brotli()
            .no_zstd()
            .no_deflate()
            .pool_max_idle_per_host(self.concurrency)
            .pool_idle_timeout(Duration::from_secs(30))
            .build()
            .context("failed to build HTTP client")
    }

    /// Deadline for reading one response body, derived from however many bytes
    /// the server says are still coming.
    ///
    /// This is a backstop against a mirror that dribbles bytes just fast enough
    /// to keep [`Self::read_stall_timeout`] from firing. Ordinary dead
    /// connections are caught far sooner by the stall timeout.
    fn body_deadline(&self, remaining: Option<u64>) -> Duration {
        match remaining {
            Some(bytes) => Duration::from_secs(bytes / self.min_bytes_per_sec)
                .clamp(self.min_body_deadline, self.max_body_deadline),
            None => self.max_body_deadline,
        }
    }
}

/// Batches the ~16 KiB chunks reqwest hands back, since every `tokio::fs`
/// write is a `spawn_blocking` round trip. Deliberately far below tokio's 2 MiB
/// `DEFAULT_MAX_BUF_SIZE` (draining a full buffer is still one dispatch either
/// way): `concurrency` downloads are live at once, and `tokio::fs::File` keeps
/// its own buffer sized to the largest write it has seen, so every extra byte
/// here is paid for twice, times the concurrency.
const WRITE_BUFFER: usize = 256 * 1024;

/// How much must arrive before the byte-level bar is repositioned. Every
/// accepted `set_position` takes indicatif's global draw lock and re-renders
/// every visible bar, so a per-chunk update would serialize a hundred
/// concurrent downloads against each other. The bar's steady tick keeps it
/// animating in between.
const PROGRESS_UPDATE_BYTES: u64 = 1024 * 1024;

/// Cap on how many failures are named inline in the final error, and logged
/// individually. Each `error!` forces a full redraw of the progress bars, so
/// emitting tens of thousands of them would take longer than the downloads.
const MAX_REPORTED_FAILURES: usize = 20;

const TRANSFER_INITIAL_BACKOFF: Duration = Duration::from_secs(1);
const TRANSFER_MAX_BACKOFF: Duration = Duration::from_secs(30);

/// Attempt-bounded retry helper for package transfers. Sleeps between attempts
/// and hands the last error back to the caller once the budget is spent.
///
/// Unlike [`super::storage::retry_policy`] the backoff here is deliberately not bounded by
/// wall clock: a legitimate multi-gigabyte transfer runs for longer than any
/// sane `max_elapsed_time`, so an elapsed-time bound would make backoff refuse
/// to retry after the very first failure.
struct Retrier {
    backoff: ExponentialBackoff,
    max_attempts: usize,
    attempts_made: usize,
}

impl Retrier {
    fn new(max_attempts: usize) -> Self {
        Self {
            backoff: ExponentialBackoffBuilder::new()
                .with_initial_interval(TRANSFER_INITIAL_BACKOFF)
                .with_max_interval(TRANSFER_MAX_BACKOFF)
                .with_max_elapsed_time(None)
                .build(),
            max_attempts,
            attempts_made: 0,
        }
    }

    /// Record that an attempt failed and sleep before the next one. Returns the
    /// error unchanged (with attempt context) when no attempts remain.
    async fn wait(&mut self, filename: &str, err: anyhow::Error) -> Result<()> {
        self.attempts_made += 1;
        if self.attempts_made >= self.max_attempts {
            return Err(err.context(format!(
                "gave up on {filename} after {} attempts",
                self.max_attempts
            )));
        }
        // `max_elapsed_time` is None, so this never actually runs out.
        let delay = self
            .backoff
            .next_backoff()
            .unwrap_or(TRANSFER_INITIAL_BACKOFF);
        warn!(
            filename,
            error = format!("{err:#}"),
            ?delay,
            attempt = self.attempts_made + 1,
            "retrying after failure"
        );
        tokio::time::sleep(delay).await;
        Ok(())
    }
}

/// A temp file accumulating the bytes of one package. Lives across retry
/// attempts so a stall 1 GiB into a 1.3 GiB package costs the stall rather than
/// the gigabyte.
struct PartialDownload {
    // Held but never read: dropping it unlinks the file.
    _tmp: NamedTempFile,
    path: PathBuf,
}

impl PartialDownload {
    async fn create() -> Result<Self> {
        // NamedTempFile::new() does blocking filesystem I/O.
        let tmp = tokio::task::spawn_blocking(NamedTempFile::new)
            .await
            .context("spawn_blocking failed")?
            .context("failed to create temp file")?;
        let path = tmp.path().to_owned();
        Ok(Self { _tmp: tmp, path })
    }

    /// How much of the package is already on disk, and therefore where the next
    /// attempt resumes. The filesystem is the only trustworthy answer after a
    /// failure: a write cancelled mid-chunk leaves the file longer than the loop
    /// that issued it managed to count.
    async fn len(&self) -> Result<u64> {
        Ok(tokio::fs::metadata(&self.path)
            .await
            .with_context(|| format!("failed to stat {}", self.path.display()))?
            .len())
    }

    /// Discard everything downloaded so far.
    async fn reset(&self) -> Result<()> {
        tokio::fs::File::create(&self.path)
            .await
            .with_context(|| format!("failed to truncate {}", self.path.display()))?;
        Ok(())
    }
}

/// An HTTP client paired with the settings that govern how it is used.
struct Downloader {
    client: reqwest::Client,
    settings: PackageDownloadSettings,
}

impl Downloader {
    fn new(settings: PackageDownloadSettings) -> Result<Self> {
        Ok(Self {
            client: settings.http_client()?,
            settings,
        })
    }

    /// Ask for the bytes of `url` that `dst` does not already have, returning
    /// the response to read them from and the offset they start at.
    ///
    /// Range support is probed by simply asking for one: a server that honours
    /// `Range: bytes=N-` answers `206 Partial Content`, one that does not answers
    /// `200 OK` with the whole body and we start over. That is more reliable than
    /// trusting `Accept-Ranges`, which plenty of mirrors omit while still
    /// supporting ranges.
    async fn ranged_response(
        &self,
        url: &str,
        filename: &str,
        dst: &PartialDownload,
    ) -> Result<(reqwest::Response, u64)> {
        let resume_from = dst.len().await?;
        let mut request = self.client.get(url);
        if resume_from > 0 {
            request = request.header(RANGE, format!("bytes={resume_from}-"));
        }

        let response = request
            .send()
            .await
            .with_context(|| format!("failed to request {url}"))?;

        // We asked for a range the server considers past EOF, so our partial file
        // disagrees with the server's copy. Throw it away and start over.
        if response.status() == StatusCode::RANGE_NOT_SATISFIABLE {
            dst.reset().await?;
            bail!("{url} rejected a range request at offset {resume_from}");
        }

        let response = response
            .error_for_status()
            .with_context(|| format!("HTTP error downloading {url}"))?;

        if resume_from > 0 && response.status() != StatusCode::PARTIAL_CONTENT {
            warn!(
                filename,
                resume_from, "server ignored range request, restarting from byte 0"
            );
            dst.reset().await?;
            return Ok((response, 0));
        }

        Ok((response, resume_from))
    }

    /// Stream `url` into `dst`, appending to whatever is already there.
    ///
    /// On error whatever arrived is left on disk, so the next attempt picks up
    /// from there rather than refetching it.
    async fn download_into(&self, url: &str, filename: &str, dst: &PartialDownload) -> Result<()> {
        let (mut response, base) = self.ranged_response(url, filename, dst).await?;

        // On a 206 this is the length of the remaining range, not of the package.
        let remaining = response.content_length();
        let total = remaining.map(|remaining| remaining + base);
        let deadline = self.settings.body_deadline(remaining);

        let file = tokio::fs::OpenOptions::new()
            .write(true)
            .open(&dst.path)
            .await
            .with_context(|| format!("failed to open {} for writing", dst.path.display()))?;
        // Every `tokio::fs` write is a `spawn_blocking` round trip, so writing
        // each HTTP chunk straight through would cost tens of thousands of
        // dispatches per gigabyte, times `concurrency` downloads at once.
        let mut file = tokio::io::BufWriter::with_capacity(WRITE_BUFFER, file);
        file.seek(std::io::SeekFrom::Start(base))
            .await
            .context("failed to seek in temp file")?;

        let pb = total
            .filter(|total| *total >= self.settings.byte_progress_min_bytes)
            .map(|total| {
                let pb = progress::bytes_bar(total, format!("Downloading {filename}"));
                pb.set_position(base);
                pb
            });

        // Counts what *this attempt* transferred; add `base` for the file offset.
        let mut written = 0u64;
        let start = Instant::now();
        let copy = async {
            let mut drawn = 0u64;
            while let Some(chunk) = response
                .chunk()
                .await
                .with_context(|| format!("failed to read chunk from {url}"))?
            {
                file.write_all(&chunk)
                    .await
                    .context("failed to write chunk to temp file")?;
                written += chunk.len() as u64;
                if let Some(pb) = &pb
                    && written - drawn >= PROGRESS_UPDATE_BYTES
                {
                    drawn = written;
                    pb.set_position(base + written);
                }
            }
            file.flush().await.context("failed to flush temp file")?;
            Ok::<_, anyhow::Error>(())
        };

        let timed_out = tokio::time::timeout(deadline, copy).await;
        if let Some(pb) = &pb {
            pb.finish_and_clear();
        }

        // However the copy ended — a mid-body error, or the deadline cancelling
        // a write outright — the buffer can still hold bytes we already received.
        // Push them to disk so the next attempt resumes past them instead of
        // refetching them. `BufWriter` tracks its own partial-write cursor, so
        // flushing after a cancelled write picks up exactly where it stopped.
        if !file.buffer().is_empty() {
            file.flush()
                .await
                .context("failed to flush buffered download bytes")?;
        }
        // A cancelled `write_all` can leave the file longer than this, since the
        // counter only advances once a whole chunk is accepted. It is a floor,
        // used only for reporting; the resume offset comes from the filesystem.
        let got = base + written;

        match timed_out {
            Ok(result) => result?,
            Err(_) => bail!(
                "timed out downloading {url}: got {got} of {} bytes in {:?}, over this response's {deadline:?} deadline",
                total.map_or_else(|| "?".to_owned(), |total| total.to_string()),
                start.elapsed(),
            ),
        }

        // A body that ends early is a transient mirror failure, not a corrupt
        // package. Reporting it as such gets it resumed rather than restarted.
        if let Some(total) = total
            && got != total
        {
            bail!("{url} delivered {got} bytes, expected {total}");
        }

        let elapsed = start.elapsed();
        if elapsed > self.settings.slow_transfer_warn {
            warn!(filename, bytes = written, ?elapsed, "slow download");
        }
        Ok(())
    }

    /// Download a package, verify its checksum against expected, then upload flat with retries.
    async fn download_and_upload(
        &self,
        storage: &dyn Storage,
        download_url: &str,
        filename: &str,
        expected_checksums: &snapshot_common::Checksums,
    ) -> Result<UrlWithChecksums> {
        let file_start = Instant::now();

        // Download + verify as one retry unit: a corrupt download is only
        // recoverable by re-downloading.
        let dst = PartialDownload::create().await?;
        let mut retrier = Retrier::new(self.settings.download_attempts);
        let computed_checksums = loop {
            let fetched = async {
                self.download_into(download_url, filename, &dst).await?;
                snapshot_common::Checksums::from_file_async(dst.path.clone()).await
            }
            .await;

            let computed = match fetched {
                Ok(computed) => computed,
                // Whatever arrived is still on disk, so the next attempt resumes.
                Err(e) => {
                    retrier.wait(filename, e).await?;
                    continue;
                }
            };

            // Verify checksums before upload (supply-chain integrity). A mismatch
            // means the bytes on disk are worthless, so the next attempt must start
            // from scratch rather than resume.
            match expected_checksums.verify_against(&computed) {
                Ok(()) => break computed,
                Err(e) => {
                    dst.reset().await?;
                    retrier.wait(filename, anyhow::Error::from(e)).await?;
                }
            }
        };

        // Upload as a separate retry unit so a flaky upload doesn't force us to
        // re-download the (already verified) file. Checksums are reused from the
        // verify above so storage doesn't re-hash the file.
        let mut retrier = Retrier::new(self.settings.upload_attempts);
        let result = loop {
            let up_start = Instant::now();
            let attempt = storage
                .store_flat_with_checksums(&dst.path, &computed_checksums)
                .await
                .with_context(|| format!("failed to upload flat {}", dst.path.display()));
            match attempt {
                Ok(result) => {
                    let up_elapsed = up_start.elapsed();
                    if up_elapsed > self.settings.slow_transfer_warn {
                        warn!(filename, ?up_elapsed, "slow upload");
                    }
                    break result;
                }
                Err(e) => retrier.wait(filename, e).await?,
            }
        };

        debug!(
            filename,
            total_elapsed = ?file_start.elapsed(),
            "download+upload complete"
        );
        Ok(result)
    }

    /// Ensure one package's blob is in storage, downloading it from the mirror if
    /// it is not already there. Returns `None` when the blob turned up in storage
    /// between the blob-status check and now.
    async fn snapshot_one_package(
        &self,
        storage: &dyn Storage,
        base_url: &str,
        entry: &Entry,
    ) -> Result<Option<UrlWithChecksums>> {
        // Another process may have uploaded this between blob-status and now. If it
        // did, just extend the TTL instead of downloading.
        match storage.blob_status(&entry.checksums).await? {
            status @ (BlobStatus::Fresh | BlobStatus::ExpiringSoon) => {
                info!(
                    filename = entry.filename,
                    "blob already exists ({status:?}), extending its ttl",
                );
                storage.extend_ttl(&entry.checksums).await?;
                return Ok(None);
            }
            BlobStatus::Missing => {}
        }

        let download_url = format!("{}/{}", base_url, entry.filename);
        debug!(
            filename = entry.filename,
            download_url, "downloading package blob"
        );

        self.download_and_upload(storage, &download_url, &entry.filename, &entry.checksums)
            .await
            .map(Some)
    }
}

#[derive(Parser, Debug)]
pub(crate) struct Packages {
    #[clap(long)]
    out: PathBuf,
    #[clap(long)]
    storage: Json<StorageConfig>,
    /// JSON file containing the output of blob-status (missing + expiring_soon).
    #[clap(long)]
    blob_status: JsonFile<BlobStatusOutput>,
    /// Path(s) to JSON files containing the full list of blob entries. After
    /// uploading missing blobs, every entry here gets a tree symlink pointing
    /// at its flat content-addressed object.
    #[clap(long)]
    all_entries: Vec<JsonFile<Vec<Entry>>>,
    /// Base URL to download packages from.
    #[clap(long)]
    base_url: String,
}

impl Packages {
    #[tracing::instrument(skip(self, fb), ret, err)]
    pub(crate) async fn run(self, fb: fbinit::FacebookInit) -> Result<()> {
        let storage = self.storage.into_inner().build(fb)?;
        let storage: Arc<dyn Storage> = storage.into();
        let blob_status = self.blob_status.into_inner();
        let all_entries: Vec<Entry> = self
            .all_entries
            .into_iter()
            .flat_map(JsonFile::into_inner)
            .collect();
        let out = snapshot_packages(storage, blob_status, &all_entries, &self.base_url).await?;
        let mut outfile = BufWriter::new(stdio_path::create(&self.out)?);
        serde_json::to_writer(&mut outfile, &out)?;
        // BufWriter swallows flush errors on drop; flush explicitly so a
        // disk-full / EIO surfaces instead of silently leaving a partial JSON.
        outfile.flush().context("failed to flush packages output")?;
        Ok(())
    }
}

/// Everything one [`download_pass`] managed to put in storage, plus whatever it
/// could not. Successes carry only the filename, so the (much larger) `Entry`
/// is dropped as soon as a package lands rather than held to the end of the pass.
#[derive(Default)]
struct PassOutcome {
    uploaded: Vec<(String, UrlWithChecksums)>,
    failed: Vec<(Entry, anyhow::Error)>,
}

/// Download and upload every entry at the given concurrency. Unlike a
/// `try_collect` this never short circuits: one unreachable package must not
/// throw away the work already done for the thousands around it.
async fn download_pass(
    downloader: &Arc<Downloader>,
    storage: &Arc<dyn Storage>,
    base_url: &str,
    entries: Vec<Entry>,
    concurrency: usize,
    label: &str,
) -> PassOutcome {
    let pb = progress::bar(entries.len(), label);
    let outcome = stream::iter(entries.into_iter().map(|entry| {
        let storage = Arc::clone(storage);
        let downloader = Arc::clone(downloader);
        let pb = pb.clone();
        async move {
            let result = downloader
                .snapshot_one_package(&*storage, base_url, &entry)
                .await;
            pb.inc(1);
            (entry, result)
        }
    }))
    .buffer_unordered(concurrency)
    .fold(
        PassOutcome::default(),
        |mut acc, (entry, result)| async move {
            match result {
                Ok(Some(uploaded)) => acc.uploaded.push((entry.filename, uploaded)),
                // Already in storage; nothing new to record.
                Ok(None) => {}
                Err(e) => acc.failed.push((entry, e)),
            }
            acc
        },
    )
    .await;
    pb.finish_with_message(label.to_owned());
    outcome
}

pub(crate) async fn snapshot_packages(
    storage: std::sync::Arc<dyn Storage>,
    blob_status: BlobStatusOutput,
    all_entries: &[Entry],
    base_url: &str,
) -> Result<Out> {
    let settings = PackageDownloadSettings::default();
    let downloader = Arc::new(Downloader::new(settings)?);

    // Extend TTLs for blobs that exist but are expiring soon
    let expiring_soon_len = blob_status.expiring_soon.len();
    info!("extending TTL for {expiring_soon_len} expiring-soon blobs");
    let expiring_pb = progress::bar(expiring_soon_len, "Extending TTL");
    stream::iter(blob_status.expiring_soon.into_iter().map(|entry| {
        let storage = Arc::clone(&storage);
        let pb = expiring_pb.clone();
        async move {
            debug!(filename = entry.filename, "extending TTL for expiring blob");
            let res = storage
                .extend_ttl(&entry.checksums)
                .await
                .with_context(|| format!("failed to extend TTL for {}", entry.filename));
            pb.inc(1);
            res?;
            Ok::<_, anyhow::Error>(())
        }
    }))
    .buffer_unordered(settings.concurrency)
    .try_collect::<Vec<_>>()
    .await?;
    expiring_pb.finish_with_message("Extended TTL");

    let entries = blob_status.missing;
    let total = entries.len();
    info!("downloading and uploading {total} package blobs");

    let mut outcome = download_pass(
        &downloader,
        &storage,
        base_url,
        entries,
        settings.concurrency,
        "Downloading & uploading packages",
    )
    .await;

    // Give the stragglers a second pass with the mirror mostly to themselves.
    // Contention from the wide first pass is a common reason a big package
    // crawls, and nothing else is competing for the pipe by the time this runs.
    // Past `max_stragglers` that reasoning no longer holds — the mirror is down,
    // not congested — and replaying everything four-wide would take far longer
    // than the pass that produced the failures.
    let stragglers = std::mem::take(&mut outcome.failed);
    if stragglers.len() > settings.max_stragglers {
        warn!(
            count = stragglers.len(),
            "too many failures to be mirror congestion, skipping the low-concurrency retry pass"
        );
        outcome.failed = stragglers;
    } else if !stragglers.is_empty() {
        warn!(
            count = stragglers.len(),
            "retrying packages that failed the first pass, at lower concurrency"
        );
        let retried = download_pass(
            &downloader,
            &storage,
            base_url,
            stragglers.into_iter().map(|(entry, _)| entry).collect(),
            settings.straggler_concurrency,
            "Retrying failed packages",
        )
        .await;
        outcome.uploaded.extend(retried.uploaded);
        outcome.failed = retried.failed;
    }

    report_failures(outcome.failed, total)?;

    let mut files = BTreeMap::new();
    let mut checksums = BTreeMap::new();
    for (key, uwc) in outcome.uploaded {
        let url = uwc.url.to_string();
        checksums.insert(url.clone(), uwc.checksums);
        files.insert(key, url);
    }

    // (Re)create tree symlinks for every known blob so the tree namespace
    // is complete and has a fresh TTL. Every entry is guaranteed to have a flat
    // object behind it: `report_failures` above bails unless all of them landed.
    info!("symlinking {} blobs into tree namespace", all_entries.len());

    // Pre-create parent directories in bulk with bounded concurrency
    let tree_keys_for_dirs: Vec<String> = all_entries.iter().map(|e| e.filename.clone()).collect();
    storage
        .ensure_tree_dirs(&tree_keys_for_dirs)
        .await
        .context("failed to pre-create tree parent directories")?;

    let symlink_pb = progress::bar(all_entries.len(), "Linking packages into tree");
    stream::iter(all_entries.iter().map(|entry| {
        let storage = Arc::clone(&storage);
        let pb = symlink_pb.clone();
        async move {
            let res = storage
                .symlink_flat_to_tree(&entry.checksums, &entry.filename)
                .await
                .with_context(|| format!("failed to symlink {} into tree", entry.filename));
            pb.inc(1);
            res
        }
    }))
    .buffer_unordered(MAX_CONCURRENT_SYMLINK)
    .try_collect::<Vec<_>>()
    .await?;
    symlink_pb.finish_with_message("Linked packages");

    Ok(Out { files, checksums })
}

/// Report every package the mirror could not serve, then fail.
///
/// A snapshot with holes in it is not a snapshot: the missing packages would
/// only surface much later, as an inscrutable failure at image build time. The
/// run has still made progress — everything that did download is durable in the
/// content-addressed flat namespace — so re-running retries just these.
fn report_failures(failures: Vec<(Entry, anyhow::Error)>, total: usize) -> Result<()> {
    if failures.is_empty() {
        return Ok(());
    }

    let mut shown = Vec::new();
    for (entry, e) in failures.iter().take(MAX_REPORTED_FAILURES) {
        error!(
            filename = entry.filename,
            error = format!("{e:#}"),
            "package download failed"
        );
        shown.push(entry.filename.as_str());
    }

    let count = failures.len();
    let mut names = shown.join(", ");
    if count > shown.len() {
        names.push_str(&format!(", and {} more", count - shown.len()));
    }

    bail!(
        "{count} of {total} packages could not be downloaded; re-run to retry just these: {names}"
    )
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;
    use std::sync::Mutex;

    use tokio::io::AsyncReadExt as _;
    use tokio::net::TcpListener;
    use tokio::net::TcpStream;

    use super::*;

    /// What the fake mirror does with one request.
    #[derive(Clone, Copy, Debug)]
    enum Behavior {
        /// Serve the requested bytes in full, answering 206 when a `Range` was
        /// asked for.
        Serve,
        /// Serve only `n` bytes and then hang up, the way a mirror dropping a
        /// connection mid-transfer does.
        Truncate(usize),
        /// Answer 200 with the whole body regardless of any `Range` header,
        /// like a mirror that does not implement range requests.
        IgnoreRange,
        /// Send the body a byte at a time with a pause between each. Slow
        /// enough to blow a body deadline while never tripping the stall
        /// timeout, which is exactly the case the deadline exists for.
        Dribble(Duration),
    }

    /// A single-threaded HTTP/1.1 server that answers a fixed script of
    /// requests. Hand-rolled rather than pulling in an HTTP server dependency;
    /// it only has to be correct for the handful of responses used here.
    struct FakeMirror {
        addr: SocketAddr,
        /// The `Range` start offset seen on each request, in order.
        ranges: Arc<Mutex<Vec<Option<u64>>>>,
    }

    impl FakeMirror {
        async fn start(body: Vec<u8>, script: Vec<Behavior>) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
            let addr = listener.local_addr().expect("local_addr");
            let ranges = Arc::new(Mutex::new(Vec::new()));

            let seen = Arc::clone(&ranges);
            tokio::spawn(async move {
                for behavior in script {
                    let (mut sock, _) = listener.accept().await.expect("accept");
                    let range = read_range(&mut sock).await;
                    seen.lock().expect("ranges lock").push(range);

                    let start = match behavior {
                        Behavior::IgnoreRange => 0,
                        _ => range.unwrap_or(0) as usize,
                    };
                    let status = match (behavior, range) {
                        (Behavior::IgnoreRange, _) | (_, None) => "200 OK",
                        _ => "206 Partial Content",
                    };
                    let chunk = &body[start..];

                    // Content-Length always advertises the full remainder, so a
                    // truncated response looks to the client exactly like a
                    // connection that died part-way through.
                    let head = format!(
                        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        chunk.len()
                    );
                    sock.write_all(head.as_bytes()).await.expect("write head");

                    // Writes past this point are best-effort: in the stall and
                    // dribble cases the client is *expected* to give up and hang
                    // up on us, and a broken pipe here is the test passing.
                    match behavior {
                        Behavior::Dribble(delay) => {
                            for byte in chunk {
                                if sock.write_all(&[*byte]).await.is_err() {
                                    break;
                                }
                                let _ = sock.flush().await;
                                tokio::time::sleep(delay).await;
                            }
                        }
                        Behavior::Truncate(n) => {
                            let _ = sock.write_all(&chunk[..n.min(chunk.len())]).await;
                            let _ = sock.flush().await;
                        }
                        Behavior::Serve | Behavior::IgnoreRange => {
                            let _ = sock.write_all(chunk).await;
                            let _ = sock.flush().await;
                        }
                    }
                }
            });

            Self { addr, ranges }
        }

        fn url(&self) -> String {
            format!("http://{}/pkg.deb", self.addr)
        }

        fn ranges(&self) -> Vec<Option<u64>> {
            self.ranges.lock().expect("ranges lock").clone()
        }
    }

    async fn read_range(sock: &mut TcpStream) -> Option<u64> {
        let mut head = Vec::new();
        let mut byte = [0u8; 1];
        while !head.ends_with(b"\r\n\r\n") {
            if sock.read_exact(&mut byte).await.is_err() {
                break;
            }
            head.push(byte[0]);
        }
        String::from_utf8_lossy(&head)
            .to_lowercase()
            .lines()
            .find_map(|line| {
                line.strip_prefix("range: bytes=")?
                    .trim_end_matches('-')
                    .parse()
                    .ok()
            })
    }

    /// Deterministic pseudo-random bytes, so a resumed file that stitched the
    /// two halves together at the wrong offset does not accidentally compare
    /// equal.
    fn body(len: usize) -> Vec<u8> {
        (0..len).map(|i| (i.wrapping_mul(31) % 251) as u8).collect()
    }

    async fn contents(dst: &PartialDownload) -> Vec<u8> {
        tokio::fs::read(&dst.path).await.expect("read temp file")
    }

    #[tokio::test]
    async fn downloads_a_whole_body() {
        let expected = body(4096);
        let mirror = FakeMirror::start(expected.clone(), vec![Behavior::Serve]).await;
        let downloader = Downloader::new(PackageDownloadSettings::default()).expect("downloader");
        let dst = PartialDownload::create().await.expect("temp file");

        downloader
            .download_into(&mirror.url(), "pkg.deb", &dst)
            .await
            .expect("a complete response should download cleanly");

        assert_eq!(contents(&dst).await, expected);
        assert_eq!(dst.len().await.expect("len"), 4096);
        assert_eq!(
            mirror.ranges(),
            vec![None],
            "a fresh download must not ask for a range"
        );
    }

    #[tokio::test]
    async fn resumes_from_where_a_truncated_download_died() {
        let expected = body(4096);
        let mirror = FakeMirror::start(
            expected.clone(),
            vec![Behavior::Truncate(1000), Behavior::Serve],
        )
        .await;
        let downloader = Downloader::new(PackageDownloadSettings::default()).expect("downloader");
        let dst = PartialDownload::create().await.expect("temp file");

        downloader
            .download_into(&mirror.url(), "pkg.deb", &dst)
            .await
            .expect_err("a body that stops short of Content-Length must be an error");
        assert_eq!(
            dst.len().await.expect("len"),
            1000,
            "the delivered prefix must be kept"
        );

        downloader
            .download_into(&mirror.url(), "pkg.deb", &dst)
            .await
            .expect("the second attempt should pick up the rest");

        assert_eq!(
            contents(&dst).await,
            expected,
            "the resumed file must be byte-identical to the original"
        );
        assert_eq!(
            mirror.ranges(),
            vec![None, Some(1000)],
            "the retry must ask for exactly the bytes that are missing"
        );
    }

    #[tokio::test]
    async fn restarts_when_the_server_ignores_the_range() {
        let expected = body(4096);
        let mirror = FakeMirror::start(
            expected.clone(),
            vec![Behavior::Truncate(1000), Behavior::IgnoreRange],
        )
        .await;
        let downloader = Downloader::new(PackageDownloadSettings::default()).expect("downloader");
        let dst = PartialDownload::create().await.expect("temp file");

        downloader
            .download_into(&mirror.url(), "pkg.deb", &dst)
            .await
            .expect_err("truncated");

        downloader
            .download_into(&mirror.url(), "pkg.deb", &dst)
            .await
            .expect("a 200 in reply to a Range request should still succeed");

        assert_eq!(
            contents(&dst).await,
            expected,
            "the partial prefix must be discarded, not prepended to the full body"
        );
        assert_eq!(mirror.ranges(), vec![None, Some(1000)]);
    }

    #[tokio::test]
    async fn a_stalled_connection_gives_up_after_read_stall_timeout() {
        // One byte, then silence for far longer than the stall timeout.
        let mirror =
            FakeMirror::start(body(4096), vec![Behavior::Dribble(Duration::from_secs(60))]).await;
        let downloader = Downloader::new(PackageDownloadSettings {
            read_stall_timeout: Duration::from_millis(200),
            ..Default::default()
        })
        .expect("downloader");
        let dst = PartialDownload::create().await.expect("temp file");

        let start = Instant::now();
        downloader
            .download_into(&mirror.url(), "pkg.deb", &dst)
            .await
            .expect_err("a connection that stops delivering bytes must not hang forever");

        assert!(
            start.elapsed() < Duration::from_secs(10),
            "should give up shortly after the 200ms stall timeout, took {:?}",
            start.elapsed()
        );
        assert_eq!(
            dst.len().await.expect("len"),
            1,
            "the byte that did arrive is kept to resume"
        );
    }

    #[tokio::test]
    async fn a_dribbling_server_gives_up_on_the_body_deadline() {
        // Every individual read lands well inside the stall timeout, so only the
        // Content-Length-derived deadline can catch this.
        let mirror = FakeMirror::start(
            body(4096),
            vec![Behavior::Dribble(Duration::from_millis(20))],
        )
        .await;
        let downloader = Downloader::new(PackageDownloadSettings {
            read_stall_timeout: Duration::from_secs(30),
            min_body_deadline: Duration::from_millis(300),
            max_body_deadline: Duration::from_millis(300),
            ..Default::default()
        })
        .expect("downloader");
        let dst = PartialDownload::create().await.expect("temp file");

        let start = Instant::now();
        let err = downloader
            .download_into(&mirror.url(), "pkg.deb", &dst)
            .await
            .expect_err("a transfer too slow to ever finish must be abandoned");

        assert!(
            start.elapsed() < Duration::from_secs(10),
            "the body deadline should fire, not the 30s stall timeout, but took {:?}",
            start.elapsed()
        );
        assert!(
            format!("{err:#}").contains("timed out downloading"),
            "should report a deadline overrun, got: {err:#}"
        );
    }

    #[tokio::test]
    async fn a_slow_but_moving_transfer_is_not_killed() {
        // Same dribble rate as above, but with the production-shaped policy:
        // the deadline scales off Content-Length instead of being pinned short,
        // so a slow-yet-progressing mirror is allowed to finish.
        let expected = body(256);
        let mirror = FakeMirror::start(
            expected.clone(),
            vec![Behavior::Dribble(Duration::from_millis(2))],
        )
        .await;
        let downloader = Downloader::new(PackageDownloadSettings::default()).expect("downloader");
        let dst = PartialDownload::create().await.expect("temp file");

        downloader
            .download_into(&mirror.url(), "pkg.deb", &dst)
            .await
            .expect("a slow transfer that keeps making progress must be allowed to finish");

        assert_eq!(contents(&dst).await, expected);
    }

    fn entry(filename: &str) -> Entry {
        serde_json::from_value(serde_json::json!({
            "filename": filename,
            "checksums": {"sha256": "0".repeat(64)},
        }))
        .expect("test entry should deserialize")
    }

    #[test]
    fn body_deadline_scales_with_content_length() {
        let settings = PackageDownloadSettings::default();
        // 1313 MiB, the size of the pathological ubuntu 0ad-data package, at
        // the 256 KiB/s floor works out to a little under 90 minutes.
        let big = settings.body_deadline(Some(1313 * 1024 * 1024));
        assert_eq!(
            big,
            Duration::from_secs(1313 * 4),
            "1313 MiB / 256 KiB/s should be 5252s"
        );
        assert!(
            big < settings.max_body_deadline,
            "and should stay under the cap"
        );

        // A small package must not get a deadline of a few seconds just
        // because it is small.
        assert_eq!(
            settings.body_deadline(Some(1024)),
            settings.min_body_deadline,
            "tiny packages are clamped up to the floor"
        );

        // Nothing to scale by without a Content-Length.
        assert_eq!(settings.body_deadline(None), settings.max_body_deadline);

        // Absurdly large bodies are clamped rather than allowed to run forever.
        assert_eq!(
            settings.body_deadline(Some(u64::MAX)),
            settings.max_body_deadline
        );
    }

    #[test]
    fn no_failures_is_never_an_error() {
        report_failures(Vec::new(), 10).expect("an empty failure list is always fine");
    }

    #[test]
    fn any_failure_fails_the_snapshot() {
        let failures = vec![(entry("pool/a.deb"), anyhow::anyhow!("timed out"))];
        let err = report_failures(failures, 10)
            .expect_err("a package the mirror could not serve must fail the snapshot");
        let msg = format!("{err:#}");
        assert!(
            msg.contains("1 of 10 packages"),
            "error should say how many of how many failed: {msg}"
        );
        assert!(
            msg.contains("pool/a.deb"),
            "error should name the failure: {msg}"
        );
    }

    #[test]
    fn long_failure_lists_are_elided() {
        let failures: Vec<_> = (0..MAX_REPORTED_FAILURES + 5)
            .map(|i| {
                (
                    entry(&format!("pool/{i}.deb")),
                    anyhow::anyhow!("timed out"),
                )
            })
            .collect();
        let err = report_failures(failures, 100).expect_err("still fatal");
        let msg = format!("{err:#}");
        assert!(
            msg.contains("and 5 more"),
            "the tail should be elided rather than dumping every name: {msg}"
        );
    }
}
