/*
 * Copyright (c) Meta Platforms, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#![deny(warnings)]

use std::ffi::OsStr;
use std::ffi::OsString;
use std::path::PathBuf;

use anyhow::Context;
use futures::Stream;
use mio::Events;
use mio::Interest;
use mio::Poll;
use mio::Token;
use slog::debug;
use slog::Logger;
use thiserror::Error;

pub mod device;
pub use device::Device;
pub use device::DeviceType;
pub use device::SpecificDevice;

#[derive(Debug, Error)]
pub enum Error {
    #[error(transparent)]
    Udev(#[from] std::io::Error),
    #[error("failed during async setup: {0:?}")]
    Async(anyhow::Error),
    #[error("failed to lookup device details for {path:?}: {error}")]
    Lookup {
        path: PathBuf,
        error: nix::errno::Errno,
    },
    #[error("{0:?} is not a device node")]
    NotADevice(PathBuf),
    #[error(transparent)]
    Specialization(#[from] device::SpecializationError),
}

pub type Result<R> = std::result::Result<R, Error>;

/// Kernel subsystem as reported by udev.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Subsystem {
    Block,
    Other(OsString),
    None,
}

impl From<&OsStr> for Subsystem {
    fn from(s: &OsStr) -> Self {
        if s == OsStr::new("block") {
            Self::Block
        } else {
            Self::Other(s.into())
        }
    }
}

/// An event from udev, or a virtual event while discovering already-attached
/// devices.
/// This is not an exhaustive list and should be added to as necessary.
#[derive(Debug, Clone)]
pub enum Event {
    Added(Device),
    Changed(Device),
    Removed(Device),
}

impl Event {
    pub fn device(&self) -> &Device {
        match self {
            Self::Added(d) | Self::Changed(d) | Self::Removed(d) => d,
        }
    }

    pub fn into_device(self) -> Device {
        match self {
            Self::Added(d) | Self::Changed(d) | Self::Removed(d) => d,
        }
    }

    /// Return a [Device] only if it's currently attached (in other words, any
    /// event types except Removed)
    pub fn as_attached_device(&self) -> Option<&Device> {
        match self {
            Self::Added(d) | Self::Changed(d) => Some(d),
            Self::Removed(_) => None,
        }
    }

    /// Return a [Device] only if it's currently attached (in other words, any
    /// event types except Removed)
    pub fn into_attached_device(self) -> Option<Device> {
        match self {
            Self::Added(d) | Self::Changed(d) => Some(d),
            Self::Removed(_) => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct StreamOpts {
    /// Generate [Event]s for all currently-attached devices
    pub enumerate_attached: bool,
    /// Listen indefinitely for any new [Event]s coming from udev.
    pub listen: bool,
    /// Log all enumerated and received events here
    pub logger: Option<Logger>,
}

impl Default for StreamOpts {
    fn default() -> Self {
        Self {
            enumerate_attached: true,
            listen: true,
            logger: None,
        }
    }
}

pub async fn stream(opts: StreamOpts) -> Result<impl Stream<Item = Event>> {
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();

    if opts.listen {
        let thread_logger = opts.logger.clone();
        let thread_tx = tx.clone();
        std::thread::spawn(move || {
            if let Err(e) = udev_stream_thread_main(thread_logger, ready_tx.clone(), thread_tx) {
                ready_tx
                    .send(Some(e))
                    .expect("failed to send error back to main thread");
            }
        });
    } else {
        ready_tx
            .send(None)
            .context("while sending ready event")
            .map_err(Error::Async)?;
    }

    // Wait for the socket reading thread to confirm that it's ready before
    // enumerating existing devices. We may get duplicate events for devices
    // here if they are being attached right now, but that's better than missing
    // them
    if let Some(internal_error) = ready_rx
        .recv()
        .context("did not get ready event")
        .map_err(Error::Async)?
    {
        return Err(internal_error);
    }
    // The socket thread is ready, we can enumerate all devices in /sys now
    if opts.enumerate_attached {
        let mut enumerator = udev::Enumerator::new()?;

        for udev_dev in enumerator.scan_devices()? {
            let dev = Device::from(&udev_dev);
            if let Some(l) = &opts.logger {
                debug!(l, "sending enumerated event {:?}", &dev);
            }
            tx.send(Event::Added(dev))
                .context("while sending enumerated event")
                .map_err(Error::Async)?;
        }
    }

    Ok(tokio_stream::wrappers::UnboundedReceiverStream::new(rx))
}

fn udev_stream_thread_main(
    logger: Option<Logger>,
    ready_tx: std::sync::mpsc::Sender<Option<Error>>,
    tx: tokio::sync::mpsc::UnboundedSender<Event>,
) -> Result<()> {
    let monitor_builder = udev::MonitorBuilder::new()?;
    let mut socket = monitor_builder.listen()?;

    let mut poll = Poll::new()?;
    let mut events = Events::with_capacity(10240);

    poll.registry().register(
        &mut socket,
        Token(0),
        Interest::READABLE | Interest::WRITABLE,
    )?;

    ready_tx.send(None).expect("ready_rx receiver is hung up");

    loop {
        poll.poll(&mut events, None)?;

        for event in &events {
            if event.token() == Token(0) && event.is_writable() {
                for event in socket.clone() {
                    let udev_dev = event.device();
                    let dev = Device::from(&udev_dev);
                    if let Some(ref l) = logger {
                        debug!(l, "received udev event {} {:?}", event.event_type(), dev)
                    }
                    let event = match event.event_type() {
                        udev::EventType::Add => Some(Event::Added(dev)),
                        udev::EventType::Change => Some(Event::Changed(dev)),
                        udev::EventType::Remove => Some(Event::Removed(dev)),
                        _ => None,
                    };
                    if let Some(e) = event {
                        // if the channel is closed, just terminate this thread
                        if tx.send(e).is_err() {
                            return Ok(());
                        };
                    }
                }
            }
        }
    }
}

pub fn blocking_stream(opts: StreamOpts) -> Result<impl Iterator<Item = Event>> {
    let (err_tx, err_rx) = std::sync::mpsc::channel();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        match futures::executor::block_on(crate::stream(opts)) {
            Ok(stream) => {
                err_tx.send(None).expect("failed to send ready message");
                for event in futures::executor::block_on_stream(stream) {
                    tx.send(event).expect("failed to send event");
                }
            }
            Err(e) => {
                err_tx
                    .send(Some(e))
                    .expect("failed to pass error to parent thread");
            }
        };
    });
    match err_rx
        .recv()
        .context("async thread dropped senders")
        .map_err(Error::Async)?
    {
        Some(err) => Err(err),
        None => Ok(rx.into_iter()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Context;
    use anyhow::Result;
    use futures::future;
    use futures::StreamExt;
    use maplit::hashset;
    use metalos_macros::vmtest;
    use std::collections::HashSet;
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;
    use std::path::Path;
    use std::path::PathBuf;
    use std::time::Duration;
    use tokio::time::timeout;

    #[vmtest]
    async fn enumerate_only_stream_ends() -> Result<()> {
        let stream = super::stream(StreamOpts {
            listen: false,
            ..Default::default()
        })
        .await?;
        // stream should finish within a reasonable time - if it was listening
        // on the socket, it would last forever
        let events: Vec<_> = timeout(Duration::from_secs(1), stream.collect()).await?;
        assert!(!events.is_empty());
        Ok(())
    }

    fn setup_loopback_with_gpt() -> Result<PathBuf> {
        const TOTAL_BYTES: usize = 1024 * 64;
        let mut device = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .truncate(true)
            .create(true)
            .open("/tmp/loop.img")?;
        device.set_len(TOTAL_BYTES as u64)?;

        let mbr = gpt::mbr::ProtectiveMBR::with_lb_size(
            u32::try_from((TOTAL_BYTES / 512) - 1).unwrap_or(0xFF_FF_FF_FF),
        );
        mbr.overwrite_lba0(&mut device)
            .context("failed to write MBR")?;

        let mut gdisk = gpt::GptConfig::default()
            .initialized(false)
            .writable(true)
            .logical_block_size(gpt::disk::LogicalBlockSize::Lb512)
            .create_from_device(Box::new(device), None)
            .context("failed to create GptDisk")?;

        gdisk
            .update_partitions(std::collections::BTreeMap::<u32, gpt::partition::Partition>::new())
            .context("failed to initialize blank partition table")?;

        gdisk
            .add_partition("test1", 1024 * 12, gpt::partition_types::BASIC, 0, None)
            .context("failed to add test1 partition")?;
        gdisk
            .add_partition("test2", 1024 * 18, gpt::partition_types::LINUX_FS, 0, None)
            .context("failed to add test2 partition")?;
        gdisk.write_inplace()?;

        let mut output = std::process::Command::new("losetup")
            .arg("--find")
            .arg("--show")
            .arg("--partscan")
            .arg("/tmp/loop.img")
            .output()
            .context("while running losetup")?;

        assert_eq!(output.stdout.pop(), Some(b'\n'));
        Ok(PathBuf::from(OsString::from_vec(output.stdout)))
    }

    #[vmtest]
    async fn discovers_new_loop() -> Result<()> {
        let log = slog::Logger::root(slog_glog_fmt::default_drain(), slog::o!());
        let stream = super::stream(StreamOpts {
            enumerate_attached: false,
            logger: Some(log),
            ..Default::default()
        })
        .await?
        .filter_map(|e| future::ready(e.into_attached_device()))
        .filter(|dev| future::ready(dev.subsystem() == &Subsystem::Block))
        .filter(|dev| future::ready(dev.device_type() == &DeviceType::Partition))
        .filter_map(|dev| future::ready(device::Partition::try_from(dev).ok()));
        let loopback_dev_path = setup_loopback_with_gpt()?;
        let loopback_dev = device::Disk::from_path(&loopback_dev_path)?;
        let loop_partitions: Vec<_> = timeout(
            Duration::from_secs(5),
            stream
                .filter(|dev| future::ready(dev.parent().map_or(false, |p| *p == loopback_dev)))
                .take(2)
                .collect(),
        )
        .await?;
        let loop_partition_devs: HashSet<_> = loop_partitions
            .iter()
            .map(|dev| dev.path().to_str().unwrap().to_string())
            .collect();
        assert_eq!(
            hashset! {
                format!("{}p1", loopback_dev_path.to_str().unwrap()),
                format!("{}p2", loopback_dev_path.to_str().unwrap()),
            },
            loop_partition_devs
        );
        Ok(())
    }

    #[vmtest]
    async fn finds_disk_by_serial() -> Result<()> {
        let log = slog::Logger::root(slog_glog_fmt::default_drain(), slog::o!());
        let stream = super::stream(StreamOpts {
            listen: false,
            logger: Some(log),
            ..Default::default()
        })
        .await?;
        let disk_by_serial = timeout(
            Duration::from_secs(5),
            stream
                .filter_map(|e| future::ready(e.into_attached_device()))
                .filter(|dev| future::ready(dev.subsystem() == &Subsystem::Block))
                .filter(|dev| future::ready(dev.device_type() == &DeviceType::Disk))
                .filter_map(|dev| future::ready(device::Disk::try_from(dev).ok()))
                .filter(|disk| future::ready(disk.serial() == Some(OsStr::new("vdb"))))
                .next(),
        )
        .await?
        .context("serial not found")?;
        assert_eq!(disk_by_serial.path(), Path::new("/dev/vda"));
        Ok(())
    }

    #[vmtest]
    async fn device_by_paths() -> Result<()> {
        let vda_by_sys =
            device::Disk::from_path("/sys/devices/pci0000:00/0000:00:06.0/virtio3/block/vda")
                .context("loading vda_by_sys")?;
        assert_eq!(vda_by_sys.path(), Path::new("/dev/vda"));
        let vda_by_dev = device::Disk::from_path("/dev/vda")?;
        assert_eq!(vda_by_dev.path(), Path::new("/dev/vda"));
        assert_eq!(vda_by_dev, vda_by_sys);
        Ok(())
    }

    #[vmtest]
    fn blocking_stream_no_tokio() -> Result<()> {
        let log = slog::Logger::root(slog_glog_fmt::default_drain(), slog::o!());
        let stream = super::blocking_stream(StreamOpts {
            listen: false,
            logger: Some(log),
            ..Default::default()
        })?;
        assert!(stream.count() > 0);
        Ok(())
    }

    #[vmtest]
    async fn blocking_stream_within_tokio_runtime() -> Result<()> {
        let log = slog::Logger::root(slog_glog_fmt::default_drain(), slog::o!());
        let stream = super::blocking_stream(StreamOpts {
            listen: false,
            logger: Some(log),
            ..Default::default()
        })?;
        assert!(stream.count() > 0);
        Ok(())
    }
}
