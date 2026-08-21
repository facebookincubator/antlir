/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::io::IsTerminal;
use std::io::Write;
use std::sync::OnceLock;
use std::time::Duration;

use indicatif::MultiProgress;
use indicatif::ProgressBar;
use indicatif::ProgressStyle;
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::fmt::writer::BoxMakeWriter;

/// Global MultiProgress instance so that tracing output and progress bars
/// cooperate – logs are printed above the bars instead of corrupting them.
static MULTI_PROGRESS: OnceLock<MultiProgress> = OnceLock::new();

pub(crate) fn multi_progress() -> MultiProgress {
    MULTI_PROGRESS.get_or_init(MultiProgress::new).clone()
}

/// Return a writer that prints log lines above the progress bars when stderr
/// is a tty. When stderr is not a tty (e.g. piped or in tests) we fall back
/// to plain stderr so that logs are never silently dropped and progress bars
/// are hidden by indicatif automatically.
pub(crate) fn tracing_writer() -> BoxMakeWriter {
    // MultiProgress is cheap to clone – it is an Arc internally.
    if std::io::stderr().is_terminal() {
        BoxMakeWriter::new(MpMakeWriter {
            mp: multi_progress(),
        })
    } else {
        BoxMakeWriter::new(std::io::stderr)
    }
}

/// Build a bar attached to the global [`MultiProgress`]. Hidden when there is
/// nothing to count, and (via indicatif's default) when stderr is not a tty.
fn styled_bar(total: u64, msg: impl Into<String>, template: &str, tick: Duration) -> ProgressBar {
    if total == 0 {
        // Hidden bar so callers can unconditionally inc/finish without branching.
        return ProgressBar::hidden();
    }
    let pb = multi_progress().add(ProgressBar::new(total));
    pb.set_style(
        ProgressStyle::with_template(template)
            .unwrap_or_else(|_| ProgressStyle::default_bar())
            .progress_chars("#>-"),
    );
    pb.set_message(msg.into());
    pb.enable_steady_tick(tick);
    pb
}

/// Create a progress bar counting `total` discrete items.
pub(crate) fn bar(total: usize, msg: impl Into<String>) -> ProgressBar {
    styled_bar(
        total as u64,
        msg,
        "{spinner:.green} {msg} [{bar:40.cyan/blue}] {pos}/{len} ({per_sec}, {eta} est remaining, {elapsed} elapsed)",
        Duration::from_millis(100),
    )
}

/// Create a byte-oriented progress bar for a single large transfer, so that a
/// multi-gigabyte package is distinguishable from a hung connection. Ticks
/// slower than [`bar`]: each steady tick is its own thread taking the global
/// draw lock, and there can be one of these per concurrent download.
pub(crate) fn bytes_bar(total: u64, msg: impl Into<String>) -> ProgressBar {
    styled_bar(
        total,
        msg,
        "{spinner:.green} {msg} [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec}, {eta} est remaining)",
        Duration::from_millis(500),
    )
}

/// Create a spinner for an operation whose total length is unknown.
/// The spinner is added to the global [`MultiProgress`].
pub(crate) fn spinner(msg: impl Into<String>) -> ProgressBar {
    let mp = multi_progress();
    let pb = mp.add(ProgressBar::new_spinner());
    pb.set_style(
        ProgressStyle::with_template("{spinner:.green} {msg} [{elapsed_precise}]")
            .unwrap_or_else(|_| ProgressStyle::default_spinner()),
    );
    pb.set_message(msg.into());
    pb.enable_steady_tick(Duration::from_millis(100));
    pb
}

const MAX_LOG_BUF: usize = 64 * 1024;

/// A [`MakeWriter`] that buffers a tracing event and prints it via
/// [`MultiProgress::println`], so log lines appear above the live bars instead
/// of corrupting them.
#[derive(Clone)]
struct MpMakeWriter {
    mp: MultiProgress,
}

impl<'a> MakeWriter<'a> for MpMakeWriter {
    type Writer = MpWriter;

    fn make_writer(&'a self) -> Self::Writer {
        MpWriter {
            mp: self.mp.clone(),
            buf: Vec::new(),
        }
    }
}

struct MpWriter {
    mp: MultiProgress,
    buf: Vec<u8>,
}

impl Write for MpWriter {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        // Cap buffer to avoid OOM on large binary log payloads
        if self.buf.len() + data.len() > MAX_LOG_BUF {
            // Flush current buffer before appending overflow
            self.flush()?;
            // If single write itself exceeds cap, truncate it
            if data.len() > MAX_LOG_BUF {
                self.buf.extend_from_slice(&data[..MAX_LOG_BUF]);
                // Force flush truncated giant line
                self.flush()?;
                return Ok(data.len());
            }
        }
        self.buf.extend_from_slice(data);
        Ok(data.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        if self.buf.is_empty() {
            return Ok(());
        }
        let text = String::from_utf8_lossy(&self.buf);
        // Use lines() to handle embedded newlines correctly and skip empty trailing line
        for line in text.lines() {
            if line.is_empty() {
                // Avoid spurious empty log lines from "\n" only messages
                continue;
            }
            // Best-effort: if there is no draw target (non-tty), fall back to
            // stderr so output is never silently dropped.
            if self.mp.println(line).is_err() {
                eprintln!("{line}");
            }
        }
        self.buf.clear();
        Ok(())
    }
}

impl Drop for MpWriter {
    fn drop(&mut self) {
        let _ = self.flush();
    }
}
