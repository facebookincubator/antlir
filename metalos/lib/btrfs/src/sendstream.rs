use crate::Result;
use crate::Subvolume;
use crate::__private::Sealed;
use anyhow::Context;
use async_compression::Level;
use async_trait::async_trait;
use bytes::Bytes;
use bytes::BytesMut;
use futures::Stream;
use futures::StreamExt;
use nix::ioctl_write_ptr;
use nix::sys::memfd::memfd_create;
use nix::sys::memfd::MemFdCreateFlag;
use std::ffi::CString;
use std::fs::File;
use std::marker::PhantomData;
use std::marker::Unpin;
use std::os::unix::io::AsRawFd;
use std::os::unix::io::FromRawFd;
use std::os::unix::io::RawFd;
use std::path::Path;
use std::path::PathBuf;
use std::process::Stdio;
use thiserror::Error;
use tokio::io::AsyncWriteExt;
use tokio::io::BufWriter;
use tokio::process::Command;
use tokio_util::codec::BytesCodec;
use tokio_util::codec::FramedRead;

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
    #[error("(de)compression error: {0}")]
    Compress(std::io::Error),
    #[error("could not find subvol name in {0:?}")]
    ParseReceived(Option<String>),
    #[error("failed to prepare tempdir: {0:?}")]
    Prepare(std::io::Error),
    #[error("failed to move received subvol from {received_path:?} to {dst:?}: {err:?}")]
    Move {
        received_path: PathBuf,
        dst: PathBuf,
        err: Box<crate::Error>,
    },
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
    /// into a `btrfs receive` process. If successfully received, the subvolume
    /// will be moved into `dst`, but will intermediately be received to a
    /// temporary directory.
    async fn receive_into(
        mut uncompressed: impl Stream<Item = std::io::Result<Bytes>> + Unpin,
        dst: impl AsRef<Path>,
    ) -> Result<Subvolume> {
        let tmpdir =
            tempfile::tempdir_in(metalos_paths::runtime::scratch()).map_err(Error::Prepare)?;
        let mut child = Command::new("/sbin/btrfs")
            .arg("receive")
            .arg(tmpdir.path())
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
        let received_path = tmpdir.path().join(subvol);
        let mut received_subvol = Subvolume::get(&received_path)?;
        let dst = dst.as_ref();
        // the subvol must first be marked as readwrite to move, then marked
        // readonly for obvious reasons
        received_subvol
            .set_readonly(false)
            .map_err(|err| Error::Move {
                received_path: received_path.clone(),
                dst: dst.to_path_buf(),
                err: Box::new(err),
            })?;
        std::fs::rename(&received_path, &dst).map_err(|err| Error::Move {
            received_path: received_path.clone(),
            dst: dst.to_path_buf(),
            err: Box::new(anyhow::Error::from(err).into()),
        })?;
        let mut received_subvol = Subvolume::get(&dst)?;
        received_subvol
            .set_readonly(true)
            .map_err(|err| Error::Move {
                received_path,
                dst: dst.to_path_buf(),
                err: Box::new(err),
            })?;

        Subvolume::get(dst)
    }
}

#[async_trait]
pub trait SendstreamExt {
    async fn receive_into(mut self, path: &Path) -> Result<Subvolume>;
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
    async fn receive_into(mut self, path: &Path) -> Result<Subvolume> {
        Self::receive_into(self.stream, path).await
    }
}

#[async_trait]
impl<S> SendstreamExt for Sendstream<Zstd, S>
where
    S: Stream<Item = std::io::Result<Bytes>> + Unpin + Send,
{
    async fn receive_into(mut self, path: &Path) -> Result<Subvolume> {
        let stream =
            tokio_util::io::ReaderStream::new(async_compression::tokio::bufread::ZstdDecoder::new(
                tokio_util::io::StreamReader::new(self.stream),
            ));
        Self::receive_into(stream, path).await
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
    use anyhow::Result;
    use metalos_macros::containertest;
    use std::path::Path;
    use systemd::Systemd;
    use systemd::WaitableSystemState;

    async fn wait_for_systemd() -> Result<()> {
        let log = slog::Logger::root(slog_glog_fmt::default_drain(), slog::o!());
        let sd = Systemd::connect(log).await?;
        sd.wait(WaitableSystemState::Starting).await?;
        Ok(())
    }

    #[containertest]
    async fn send_rw_fails() -> Result<()> {
        wait_for_systemd().await?;
        let subvol = Subvolume::root()?;
        match subvol.send_uncompressed() {
            Err(crate::Error::Sendstream(Error::SendRw)) => {}
            _ => panic!("expected SendRw error"),
        };
        Ok(())
    }

    fn setup_for_send_recv() -> Result<(Subvolume, &'static Path)> {
        let mut src = Subvolume::create("/var/tmp/src")?;
        assert_eq!(Path::new("/var/tmp/src"), src.path());
        std::fs::write("/var/tmp/src/hello", b"world\n").context("writing test file")?;
        src.set_readonly(true)?;
        Ok((src, Path::new("/run/fs/control/dst")))
    }

    fn recv_post_test(dst: &Path, recv: Subvolume) -> Result<()> {
        assert_eq!(dst, recv.path());
        assert_eq!(Path::new("/run/fs/control/dst"), recv.path());
        let s =
            std::fs::read_to_string("/run/fs/control/dst/hello").context("reading test file")?;
        assert_eq!("world\n", s);
        Ok(())
    }

    #[containertest]
    async fn simple_recv() -> Result<()> {
        wait_for_systemd().await?;
        let (src, dst) = setup_for_send_recv()?;
        let sendstream = src.send_uncompressed()?;
        let recv = sendstream.receive_into(&dst).await?;
        recv_post_test(dst, recv)
    }

    #[containertest]
    async fn zstd_recv() -> Result<()> {
        wait_for_systemd().await?;
        let (src, dst) = setup_for_send_recv()?;
        let sendstream = src.send(Level::Best)?;
        let recv = sendstream.receive_into(&dst).await?;
        recv_post_test(dst, recv)
    }

    #[containertest]
    async fn large_send_recv_uncompressed() -> Result<()> {
        wait_for_systemd().await?;
        let root = Subvolume::root()?;
        let mut snap = root.snapshot("/var/tmp/rootfs", SnapshotFlags::READONLY)?;
        snap.set_readonly(true)?;
        let sendstream = snap.send_uncompressed()?;
        let dst = Path::new("/run/fs/control/dst");
        let recv = sendstream.receive_into(dst).await?;
        assert_eq!(Path::new("/run/fs/control/dst"), recv.path());
        assert!(recv.path().join("etc/machine-id").exists());
        Ok(())
    }

    #[containertest]
    async fn large_send_recv_zstd() -> Result<()> {
        wait_for_systemd().await?;
        let root = Subvolume::root()?;
        let mut snap = root.snapshot("/var/tmp/rootfs", SnapshotFlags::READONLY)?;
        snap.set_readonly(true)?;
        let sendstream = snap.send(Level::Fastest)?;
        let dst = Path::new("/run/fs/control/dst");
        let recv = sendstream.receive_into(dst).await?;
        assert_eq!(Path::new("/run/fs/control/dst"), recv.path());
        assert!(recv.path().join("etc/machine-id").exists());
        Ok(())
    }
}
