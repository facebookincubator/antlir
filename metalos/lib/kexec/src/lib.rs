use std::ffi::CString;
use std::fs::File;
use std::io::Seek;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use nix::libc::syscall;
use nix::libc::SYS_kexec_file_load;
use slog::info;
use slog::Logger;

use metalos_host_configs::boot_config::Kernel;
use metalos_host_configs::host::HostConfig;
use metalos_host_configs::packages::Initrd;
use package_download::PackageExt;
use systemd::Systemd;

/// A small wrapper struct around the on-disk path to the kernel
/// to use for kexec
pub struct VmlinuzPath(PathBuf);

/// A small wrapper struct around the on-disk path to the kernel
/// modules to use for kexec
pub struct DiskBootModulesPath(PathBuf);

/// A small wrapper struct around the on-disk path to the initrd
/// to use for kexec
pub struct InitrdPath(PathBuf);

pub struct KexecInfo {
    vmlinuz_path: VmlinuzPath,
    disk_boot_modules_path: DiskBootModulesPath,
    initrd_path: InitrdPath,
    //TODO(T117583671): Make this structured
    cmdline: String,
}

impl KexecInfo {
    pub fn new_from_packages(kernel: &Kernel, initrd: &Initrd, cmdline: String) -> Result<Self> {
        let vmlinuz_path = VmlinuzPath(kernel.vmlinuz().context("missing vmlinuz")?);
        let disk_boot_modules_path = DiskBootModulesPath(
            kernel
                .disk_boot_modules()
                .context("missing disk-boot-modules")?,
        );
        let initrd_path = InitrdPath(initrd.on_disk().context("initrd not on disk")?);
        Ok(Self {
            vmlinuz_path,
            disk_boot_modules_path,
            initrd_path,
            cmdline,
        })
    }

    /// Prepare the joined initrd + modules image on disk and return the path to it
    pub fn prepare_initrd_image(&self) -> Result<File> {
        let mut initrd = File::open(&self.initrd_path.0)
            .with_context(|| format!("while opening initrd {}", self.initrd_path.0.display()))?;

        let mut modules = File::open(&self.disk_boot_modules_path.0).with_context(|| {
            format!(
                "while opening kernel modules {}",
                self.disk_boot_modules_path.0.display()
            )
        })?;

        // Create a temporary FD for the new initrd + modules
        let memfd = nix::sys::memfd::memfd_create(
            &CString::new("kexec_initrd_plus_modules")
                .expect("creating cstr can never fail with this static input"),
            nix::sys::memfd::MemFdCreateFlag::empty(),
        )
        .context("Failed to create memfd")?;

        let mut dest = unsafe {
            use std::os::unix::io::FromRawFd;
            File::from_raw_fd(memfd)
        };

        // Copy over the initrd + modules
        std::io::copy(&mut initrd, &mut dest)
            .context("failed to write initrd to temporary file")?;
        std::io::copy(&mut modules, &mut dest)
            .context("failed to write modules to temporary file")?;

        // Prepare the output file for our caller
        dest.rewind()
            .context("Failed to seek back to start of combined initrd file")?;

        Ok(dest)
    }

    pub fn load_file(&self, log: Logger) -> Result<()> {
        info!(
            log,
            "Loading file for kexec. kernel = {}, modules = {}, initrd = {} and cmdline = {}",
            self.vmlinuz_path.0.display(),
            self.disk_boot_modules_path.0.display(),
            self.initrd_path.0.display(),
            self.cmdline,
        );

        let kernel = File::open(&self.vmlinuz_path.0)
            .with_context(|| format!("while opening kernel {}", self.vmlinuz_path.0.display()))?;

        let initrd = self
            .prepare_initrd_image()
            .context("Failed to build initrd + modules image")?;

        let cmdline =
            CString::new(self.cmdline.clone()).context("failed to convert cmdline to CString")?;

        unsafe {
            use std::os::unix::io::IntoRawFd;
            if syscall(
                SYS_kexec_file_load,
                kernel.into_raw_fd(),
                initrd.into_raw_fd(),
                cmdline.to_bytes_with_nul().len(),
                cmdline.as_ptr(),
                0,
            ) != 0
            {
                Err(nix::errno::Errno::last())
            } else {
                Ok(())
            }
        }
        .context("while doing kexec_file_load")?;

        Ok(())
    }

    pub async fn kexec(&self, log: Logger) -> Result<()> {
        self.load_file(log.clone())
            .context("Failed to perform kexec file load")?;

        let sd = Systemd::connect(log)
            .await
            .context("Failed to connect to systemd dbus")?;

        sd.kexec().await.context("failed to call systemd.kexec")?;

        Ok(())
    }
}

impl TryFrom<&HostConfig> for KexecInfo {
    type Error = anyhow::Error;

    fn try_from(config: &HostConfig) -> Result<Self, Self::Error> {
        // TODO(T114676322) cmdline should come from the host config too
        let cmdline = std::fs::read_to_string("/proc/cmdline")
            .context("while reading kernel cmdline")?
            .trim_end()
            .to_string();

        Self::new_from_packages(
            &config.boot_config.kernel,
            &config.boot_config.initrd,
            cmdline,
        )
    }
}
