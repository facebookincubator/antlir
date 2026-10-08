/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Minimal QMP (QEMU Machine Protocol) client.
//!
//! Only what freezing and thawing a VM needs. Hand-rolled on top of
//! `serde_json` rather than pulling in the `qapi` crate, which would be a new
//! third-party dependency for a handful of commands.

use std::io::BufRead;
use std::io::BufReader;
use std::io::Write;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::path::PathBuf;
use std::thread;
use std::time::Duration;
use std::time::Instant;

use serde_json::Value;
use serde_json::json;
use thiserror::Error;
use tracing::debug;

#[derive(Error, Debug)]
pub(crate) enum QmpError {
    #[error("QMP socket {path:?} never appeared within {timeout:?}")]
    SocketTimeout { path: PathBuf, timeout: Duration },
    #[error("QMP io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("QMP protocol error: {0}")]
    Protocol(String),
    #[error("QMP command `{command}` failed: {desc}")]
    Command { command: String, desc: String },
    #[error("migration did not finish within {0:?}")]
    MigrationTimeout(Duration),
    #[error("migration ended in state `{status}`{detail}")]
    MigrationFailed { status: String, detail: String },
}

type Result<T> = std::result::Result<T, QmpError>;

pub(crate) struct Qmp {
    reader: BufReader<UnixStream>,
    writer: UnixStream,
}

impl Qmp {
    /// Connect and complete the capabilities handshake.
    pub(crate) fn connect(path: &Path, timeout: Duration) -> Result<Self> {
        let stream = Self::connect_with_retry(path, timeout)?;
        let writer = stream.try_clone()?;
        let mut qmp = Self {
            reader: BufReader::new(stream),
            writer,
        };
        // Server greeting, then leave capabilities negotiation at defaults.
        qmp.read_json()?;
        qmp.execute("qmp_capabilities", None)?;
        Ok(qmp)
    }

    fn connect_with_retry(path: &Path, timeout: Duration) -> Result<UnixStream> {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if let Ok(s) = UnixStream::connect(path) {
                return Ok(s);
            }
            thread::sleep(Duration::from_millis(50));
        }
        Err(QmpError::SocketTimeout {
            path: path.to_path_buf(),
            timeout,
        })
    }

    fn read_json(&mut self) -> Result<Value> {
        let mut line = String::new();
        if self.reader.read_line(&mut line)? == 0 {
            return Err(QmpError::Protocol("connection closed".into()));
        }
        serde_json::from_str(&line).map_err(|e| QmpError::Protocol(format!("{e}: {line}")))
    }

    /// Issue a command, skipping any asynchronous events that arrive first.
    pub(crate) fn execute(&mut self, command: &str, arguments: Option<Value>) -> Result<Value> {
        let mut msg = json!({ "execute": command });
        if let Some(args) = arguments {
            msg["arguments"] = args;
        }
        writeln!(self.writer, "{msg}")?;
        self.writer.flush()?;
        loop {
            let reply = self.read_json()?;
            if reply.get("event").is_some() {
                continue;
            }
            if let Some(err) = reply.get("error") {
                return Err(QmpError::Command {
                    command: command.to_owned(),
                    desc: err
                        .get("desc")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown error")
                        .to_owned(),
                });
            }
            return Ok(reply.get("return").cloned().unwrap_or(Value::Null));
        }
    }

    /// Enable `mapped-ram`, which lays guest RAM down at fixed offsets in the
    /// stream. That makes the frozen artifact sparse and randomly readable, so
    /// a thaw does not have to stream the whole file back sequentially.
    ///
    /// Both ends must agree, and the destination cannot negotiate it if it is
    /// already loading, which is why thawing uses `-incoming defer`.
    pub(crate) fn enable_mapped_ram(&mut self) -> Result<()> {
        self.execute(
            "migrate-set-capabilities",
            Some(json!({
                "capabilities": [{"capability": "mapped-ram", "state": true}]
            })),
        )?;
        Ok(())
    }

    pub(crate) fn stop(&mut self) -> Result<()> {
        self.execute("stop", None)?;
        Ok(())
    }

    pub(crate) fn cont(&mut self) -> Result<()> {
        self.execute("cont", None)?;
        Ok(())
    }

    pub(crate) fn quit(&mut self) -> Result<()> {
        self.execute("quit", None)?;
        Ok(())
    }

    /// Start an outgoing migration into `file`.
    pub(crate) fn migrate_to_file(&mut self, file: &Path) -> Result<()> {
        self.execute("migrate", Some(file_channels(file)))?;
        Ok(())
    }

    /// Load a previously frozen stream. Requires `-incoming defer`.
    pub(crate) fn migrate_incoming_file(&mut self, file: &Path) -> Result<()> {
        self.execute("migrate-incoming", Some(file_channels(file)))?;
        Ok(())
    }

    /// Poll until the migration reaches a terminal state.
    pub(crate) fn await_migration(&mut self, timeout: Duration) -> Result<()> {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            let info = self.execute("query-migrate", None)?;
            let status = info
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("none")
                .to_owned();
            match status.as_str() {
                "completed" => {
                    debug!("migration completed");
                    return Ok(());
                }
                "failed" | "cancelled" => {
                    let detail = info
                        .get("error-desc")
                        .and_then(Value::as_str)
                        .map(|d| format!(": {d}"))
                        .unwrap_or_default();
                    return Err(QmpError::MigrationFailed { status, detail });
                }
                _ => thread::sleep(Duration::from_millis(20)),
            }
        }
        Err(QmpError::MigrationTimeout(timeout))
    }
}

fn file_channels(file: &Path) -> Value {
    json!({
        "channels": [{
            "channel-type": "main",
            "addr": {
                "transport": "file",
                "filename": file.to_string_lossy(),
                "offset": 0,
            },
        }]
    })
}
