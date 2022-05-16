use metalos_host_configs::packages::generic::Package;
use send_events::Event;
use serde_json::json;
use std::path::Path;

pub struct RamdiskReady {}

impl From<RamdiskReady> for Event {
    fn from(_ev: RamdiskReady) -> Self {
        Self {
            name: "RAMDISK_IMAGE.READY".to_string(),
            payload: None,
        }
    }
}

pub struct FoundRootDisk<'a> {
    pub path: &'a metalos_disk::DiskDevPath,
}

impl<'a> From<FoundRootDisk<'a>> for Event {
    fn from(ev: FoundRootDisk<'a>) -> Self {
        Self {
            name: "METALOS_IMAGE.FOUND_ROOT_DISK".to_string(),
            payload: Some(json!({"path": ev.path.0})),
        }
    }
}

pub struct AppliedDiskImage<'a> {
    pub package: &'a metalos_host_configs::packages::GptRootDisk,
}

impl<'a> From<AppliedDiskImage<'a>> for Event {
    fn from(ev: AppliedDiskImage<'a>) -> Self {
        Self {
            name: "METALOS_IMAGE.APPLIED_DISK_IMAGE".to_string(),
            payload: Some(json!({
                "package": Package::from(ev.package.clone()).identifier()
            })),
        }
    }
}

pub struct MountedRootfs<'a> {
    pub source: &'a Path,
    pub target: &'a Path,
}

impl<'a> From<MountedRootfs<'a>> for Event {
    fn from(ev: MountedRootfs<'a>) -> Self {
        Self {
            name: "METALOS_IMAGE.MOUNTED_ROOTFS".to_string(),
            payload: Some(json!({"source": ev.source, "target": ev.target})),
        }
    }
}

pub struct WrittenConfig {}

impl From<WrittenConfig> for Event {
    fn from(_ev: WrittenConfig) -> Self {
        Self {
            name: "METALOS_IMAGE.WRITTEN_CONFIG".to_string(),
            payload: None,
        }
    }
}

pub struct DownloadedNextStage<'a> {
    pub kernel_package: &'a metalos_host_configs::packages::Kernel,
    pub initrd_package: &'a metalos_host_configs::packages::Initrd,
}

impl<'a> From<DownloadedNextStage<'a>> for Event {
    fn from(ev: DownloadedNextStage<'a>) -> Self {
        Self {
            name: "METALOS_IMAGE.DOWNLOADED_NEXT_STAGE".to_string(),
            payload: Some(json!({
                "kernel": Package::from(ev.kernel_package.clone()).identifier(),
                "initrd": Package::from(ev.initrd_package.clone()).identifier(),
            })),
        }
    }
}

pub struct StartingKexec<'a> {
    pub cmdline: &'a str,
}

impl<'a> From<StartingKexec<'a>> for Event {
    fn from(ev: StartingKexec<'a>) -> Self {
        Self {
            name: "METALOS_IMAGE.STARTING_KEXEC".to_string(),
            payload: Some(json!({"cmdline": ev.cmdline})),
        }
    }
}

pub struct Failure<'a> {
    pub error: &'a anyhow::Error,
}

impl<'a> From<Failure<'a>> for Event {
    fn from(ev: Failure<'a>) -> Self {
        Self {
            name: "METALOS_IMAGE.FAILURE".to_string(),
            payload: Some(json!({
                "message": format!("{}", ev.error),
                "full": format!("{:?}", ev.error),
            })),
        }
    }
}
