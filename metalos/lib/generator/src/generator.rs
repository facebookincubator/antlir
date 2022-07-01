/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::ffi::CString;
use std::ffi::OsStr;
use std::fs::File;
use std::io::Cursor;
use std::io::Seek;
use std::io::Write;
use std::os::unix::io::FromRawFd;
use std::process::ExitStatus;

use bufsize::SizeCounter;
use bytes::Bytes;
use bytes::BytesMut;
use fbthrift::binary_protocol::deserialize;
use fbthrift::binary_protocol::serialize;
use fbthrift::binary_protocol::BinaryProtocolDeserializer;
use fbthrift::binary_protocol::BinaryProtocolSerializer;
use fbthrift::Deserialize;
use fbthrift::Serialize;
use thiserror::Error;

use sandbox::sandbox;

#[derive(Debug, Error)]
pub enum Error {
    #[error("deserializing output failed: {0}")]
    Deserialize(anyhow::Error),
    #[error("preparing input fd failed: {0}")]
    PrepareInput(std::io::Error),
    #[error("sandboxing process failed: {0}")]
    Sandbox(anyhow::Error),
    #[error("spawning generator failed: {0}")]
    Spawn(std::io::Error),
    #[error("generator exited with {status}\nstdout: {stdout}\nstderr: {stderr}")]
    Eval {
        status: ExitStatus,
        stderr: String,
        stdout: String,
    },
}

pub type Result<T> = std::result::Result<T, Error>;

/// Run a MetalOS generator in a sandboxed environment. This handles spawning
/// the process, handing it the input struct via binary thrift and deserializing
/// the output.
pub fn evaluate<B, I, O>(binary: B, input: &I) -> Result<O>
where
    B: AsRef<OsStr>,
    I: Serialize<BinaryProtocolSerializer<SizeCounter>>
        + Serialize<BinaryProtocolSerializer<BytesMut>>,
    O: Deserialize<BinaryProtocolDeserializer<Cursor<Bytes>>>,
{
    let input = serialize(input);

    let mut stdin = unsafe {
        File::from_raw_fd(
            nix::sys::memfd::memfd_create(
                &CString::new("input")
                    .expect("creating cstr can never fail with this static input"),
                nix::sys::memfd::MemFdCreateFlag::empty(),
            )
            .map_err(|e| Error::PrepareInput(e.into()))?,
        )
    };
    stdin.write_all(&input).map_err(Error::PrepareInput)?;
    stdin.rewind().map_err(Error::PrepareInput)?;

    let output = sandbox(binary, Default::default())
        .map_err(Error::Sandbox)?
        .stdin(stdin)
        .output()
        .map_err(Error::Spawn)?;

    if !output.status.success() {
        return Err(Error::Eval {
            status: output.status,
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        });
    }

    deserialize(output.stdout).map_err(Error::Deserialize)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn echo() -> Result<()> {
        let input = test_if::Input {
            hello: "world".into(),
        };
        let output: test_if::Output =
            evaluate(std::env::var_os("ECHO_GENERATOR").unwrap(), &input)?;
        assert_eq!(
            output,
            test_if::Output {
                echo: "world".into(),
            }
        );
        Ok(())
    }
}
