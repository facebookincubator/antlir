use crate::{Result, Subvolume, __private::Sealed};
use anyhow::Context;
use async_compression::Level;
use async_trait::async_trait;
use bytes::{Bytes, BytesMut};
use futures::{Stream, StreamExt};
use nix::ioctl_write_ptr;
use nix::sys::memfd::{memfd_create, MemFdCreateFlag};
use std::ffi::CString;
use std::fs::File;
use std::marker::{PhantomData, Unpin};
use std::os::unix::io::{AsRawFd, FromRawFd, RawFd};
use std::process::Stdio;
use thiserror::Error;
use tokio::io::{AsyncWriteExt, BufWriter};
use tokio::process::Command;
use tokio_util::codec::{BytesCodec, FramedRead};

use btrfsutil_sys::btrfs_ioctl_send_args;

#[derive(Error, Debug)]
pub enum Error {
    #[error("cannot send rw subvol")]
    SendRw,
    #[error("error while getting next input chunk: {0}")]
    GetChunk(std::io::Error),
    #[error("writing output chunk failed: {0}")]
    WriteChunk(std::io::Error),
    #[error("btrfs-receive failed to start: {0}")]
    StartReceive(std::io::Error),
    #[error("(de)compression error {0}")]
    Compress(std::io::Error),
    #[error("could not find subvol name in {0:?}")]
    ParseReceived(Option<String>),
    #[error("btrfs receive failed: {0}")]
    Finish(anyhow::Error),
}

pub trait Compression: Sealed {}

pub struct Zstd();

impl Compression for Zstd {}
impl Sealed for Zstd {}

pub struct Uncompressed();

impl Compression for Uncompressed {}
impl Sealed for Uncompressed {}

pub struct Sendstream<C, S>
where
    C: Compression,
    S: Stream<Item = std::io::Result<Bytes>>,
{
    stream: S,
    phantom: PhantomData<C>,
}

impl<C, S> Sendstream<C, S>
where
    C: Compression,
    S: Stream<Item = std::io::Result<Bytes>> + Unpin + Send,
{
    pub fn new(stream: S) -> Self {
        Self {
            stream,
            phantom: PhantomData,
        }
    }

    /// Common pieces of `btrfs receive`, that dumps an uncompressed sendstream
    /// into a `btrfs receive` process.
    async fn receive_into(
        mut uncompressed: impl Stream<Item = std::io::Result<Bytes>> + Unpin,
        parent: &Subvolume,
    ) -> Result<Subvolume> {
        let mut child = Command::new("/sbin/btrfs")
            .arg("receive")
            .arg(parent.path())
            .stdin(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(Error::StartReceive)?;
        let mut stdin = BufWriter::new(child.stdin.take().expect("stdin is a pipe"));
        while let Some(chunk) = uncompressed.next().await {
            let chunk = chunk.map_err(Error::GetChunk)?;
            tokio::io::copy(&mut std::io::Cursor::new(chunk), &mut stdin)
                .await
                .map_err(Error::WriteChunk)?;
        }
        stdin.flush().await.map_err(Error::WriteChunk)?;
        drop(stdin);
        let output = child
            .wait_with_output()
            .await
            .context("while waiting for btrfs-receive to exit")
            .map_err(Error::Finish)?;

        let stderr = std::str::from_utf8(&output.stderr)
            .context("decoding btrfs-receive stderr")
            .map_err(Error::Finish)?;

        output
            .status
            .exit_ok()
            .context(stderr.to_string())
            .map_err(Error::Finish)?;

        let at_subvol = stderr.lines().next().ok_or(Error::ParseReceived(None))?;
        let subvol = parse_at_subvol(at_subvol.to_string())?;
        Subvolume::get(parent.path().join(subvol))
    }
}

#[async_trait]
pub trait SendstreamExt {
    async fn receive_into(mut self, parent: &Subvolume) -> Result<Subvolume>;
}

fn parse_at_subvol(at_subvol: String) -> std::result::Result<String, Error> {
    at_subvol
        .strip_prefix("At subvol ")
        .ok_or_else(|| Error::ParseReceived(Some(at_subvol.clone())))
        .map(|s| s.trim().to_string())
}

#[async_trait]
impl<S> SendstreamExt for Sendstream<Uncompressed, S>
where
    S: Stream<Item = std::io::Result<Bytes>> + Unpin + Send,
{
    async fn receive_into(mut self, parent: &Subvolume) -> Result<Subvolume> {
        Self::receive_into(self.stream, parent).await
    }
}

#[async_trait]
impl<S> SendstreamExt for Sendstream<Zstd, S>
where
    S: Stream<Item = std::io::Result<Bytes>> + Unpin + Send,
{
    async fn receive_into(mut self, parent: &Subvolume) -> Result<Subvolume> {
        let stream =
            tokio_util::io::ReaderStream::new(async_compression::tokio::bufread::ZstdDecoder::new(
                tokio_util::io::StreamReader::new(self.stream),
            ));
        Self::receive_into(stream, parent).await
    }
}

ioctl_write_ptr!(btrfs_ioc_send, 0x94, 38, btrfs_ioctl_send_args);

impl Subvolume {
    fn create_uncompressed_memfd(&self) -> Result<RawFd> {
        if !self.is_readonly() {
            return Err(crate::Error::Sendstream(Error::SendRw));
        }
        let memfd = memfd_create(
            &CString::new(format!("btrfs-send/subvolid={}", self.id)).unwrap(),
            MemFdCreateFlag::MFD_CLOEXEC,
        )
        .with_context(|| format!("failed to create memfd to send subvolid={}", self.id))?;
        let args = btrfs_ioctl_send_args {
            send_fd: memfd as i64,
            // clone_sources and parent correspond to `-c` and `-p`, which we do
            // not support
            clone_sources_count: 0,
            clone_sources: std::ptr::null_mut(),
            parent_root: 0,
            flags: 0,
            reserved: [0; 4],
        };
        let f = File::open(self.path())
            .with_context(|| format!("failed to open subvol={}", self.path().display()))?;
        unsafe { btrfs_ioc_send(f.as_raw_fd(), &args) }
            .with_context(|| format!("while calling btrfs_ioc_send(subvolid={})", self.id))?;

        Ok(memfd)
    }

    /// Create an uncompressed sendstream
    pub fn send_uncompressed(
        &self,
    ) -> Result<Sendstream<Uncompressed, impl Stream<Item = std::io::Result<Bytes>>>> {
        let memfd = self.create_uncompressed_memfd()?;
        let sendstream_memfd = unsafe { tokio::fs::File::from_raw_fd(memfd) };
        let stream =
            FramedRead::new(sendstream_memfd, BytesCodec::new()).map(|r| r.map(BytesMut::freeze));
        Ok(Sendstream::new(stream))
    }

    /// Create a zstd-compressed [Sendstream].
    pub fn send(
        &self,
        level: Level,
    ) -> Result<Sendstream<Zstd, impl Stream<Item = std::io::Result<Bytes>>>> {
        let memfd = self.create_uncompressed_memfd()?;
        let sendstream_memfd = unsafe { tokio::fs::File::from_raw_fd(memfd) };
        let stream =
            FramedRead::new(sendstream_memfd, BytesCodec::new()).map(|r| r.map(BytesMut::freeze));

        let stream = tokio_util::io::ReaderStream::new(
            async_compression::tokio::bufread::ZstdEncoder::with_quality(
                tokio_util::io::StreamReader::new(stream),
                level,
            ),
        );

        Ok(Sendstream::new(stream))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SnapshotFlags;
    use metalos_macros::containertest;
    use std::path::Path;

    #[containertest]
    fn send_rw_fails() -> Result<()> {
        let subvol = Subvolume::root()?;
        match subvol.send_uncompressed() {
            Err(crate::Error::Sendstream(Error::SendRw)) => {}
            _ => panic!("expected SendRw error"),
        };
        Ok(())
    }

    fn setup_for_send_recv() -> Result<(Subvolume, Subvolume)> {
        let mut src = Subvolume::create("/var/tmp/src")?;
        assert_eq!(Path::new("/var/tmp/src"), src.path());
        let dst = Subvolume::create("/var/tmp/dst")?;
        std::fs::write("/var/tmp/src/hello", b"world\n").context("writing test file")?;
        src.set_readonly(true)?;
        Ok((src, dst))
    }

    fn recv_post_test(dst: Subvolume, recv: Subvolume) -> Result<()> {
        assert_eq!(Path::new("/var/tmp/dst/src"), recv.path());
        let children: Vec<_> = dst.children()?.collect::<Result<_>>()?;
        assert_eq!(1, children.len());
        let s = std::fs::read_to_string("/var/tmp/dst/src/hello").context("reading test file")?;
        assert_eq!("world\n", s);
        Ok(())
    }

    #[containertest]
    async fn simple_recv() -> Result<()> {
        let (src, dst) = setup_for_send_recv()?;
        let sendstream = src.send_uncompressed()?;
        let recv = sendstream.receive_into(&dst).await?;
        recv_post_test(dst, recv)
    }

    #[containertest]
    async fn zstd_recv() -> Result<()> {
        let (src, dst) = setup_for_send_recv()?;
        let sendstream = src.send(Level::Best)?;
        let recv = sendstream.receive_into(&dst).await?;
        recv_post_test(dst, recv)
    }

    #[containertest]
    async fn large_send_recv_uncompressed() -> Result<()> {
        let root = Subvolume::root()?;
        let mut snap = root.snapshot("/var/tmp/rootfs", SnapshotFlags::READONLY)?;
        snap.set_readonly(true)?;
        let sendstream = snap.send_uncompressed()?;
        let dst = Subvolume::create("/var/tmp/rootfs-recv")?;
        let recv = sendstream.receive_into(&dst).await?;
        assert_eq!(Path::new("/var/tmp/rootfs-recv/rootfs"), recv.path());
        assert!(recv.path().join("etc/machine-id").exists());
        Ok(())
    }

    #[containertest]
    async fn large_send_recv_zstd() -> Result<()> {
        let root = Subvolume::root()?;
        let mut snap = root.snapshot("/var/tmp/rootfs", SnapshotFlags::READONLY)?;
        snap.set_readonly(true)?;
        let sendstream = snap.send(Level::Fastest)?;
        let dst = Subvolume::create("/var/tmp/rootfs-recv")?;
        let recv = sendstream.receive_into(&dst).await?;
        assert_eq!(Path::new("/var/tmp/rootfs-recv/rootfs"), recv.path());
        assert!(recv.path().join("etc/machine-id").exists());
        Ok(())
    }
}
