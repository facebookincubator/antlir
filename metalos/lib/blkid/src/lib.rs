use std::ffi::CStr;
use std::ffi::CString;
use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::path::PathBuf;

use anyhow::ensure;
use anyhow::Error;
use anyhow::Result;
use blkid_sys::blkid_do_safeprobe;
use blkid_sys::blkid_evaluate_spec;
use blkid_sys::blkid_free_probe;
use blkid_sys::blkid_new_probe_from_filename;
use blkid_sys::blkid_probe_enable_superblocks;
use blkid_sys::blkid_probe_lookup_value;
use libc::c_void;

/// Evaluate a blkid tag spec. This can be either a key-value pair like LABEL=/
/// or a full disk path like /dev/sda.
pub fn evaluate_spec(spec: impl AsRef<str>) -> Option<PathBuf> {
    let spec = CString::new(spec.as_ref()).ok()?;
    let device_ptr = unsafe { blkid_evaluate_spec(spec.as_ptr(), std::ptr::null_mut()) };
    if device_ptr.is_null() {
        return None;
    }
    let device_cstr = unsafe { CStr::from_ptr(device_ptr) };
    let device = Path::new(OsStr::from_bytes(device_cstr.to_bytes())).to_path_buf();
    unsafe { libc::free(device_ptr as *mut c_void) };
    Some(device)
}

/// Try to determine the filesystem type of a block device using libblkid. If
/// you don't know the full path to the block device ahead of time, use
/// [evaluate_spec] to find it.
pub fn probe_fstype(device: impl AsRef<Path>) -> Result<String> {
    let filename_cstr = CString::new(device.as_ref().as_os_str().as_bytes())?;
    let probe = unsafe { blkid_new_probe_from_filename(filename_cstr.as_ptr()) };
    ensure!(
        !probe.is_null(),
        "failed to create blkid probe for '{}'",
        device.as_ref().display()
    );
    let probe_result = unsafe {
        blkid_probe_enable_superblocks(probe, 1);
        blkid_do_safeprobe(probe)
    };
    match probe_result {
        0 => Ok(()),
        1 => Err(Error::msg("blkid not able to discover fstype")),
        -1 => Err(Error::msg("blkid had error trying to discover fstype")),
        -2 => Ok(()),
        _ => Err(Error::msg(format!(
            "unexpected return code from blkid_do_safeprobe: {}",
            probe_result,
        ))),
    }?;

    let mut fstype: *const libc::c_char = std::ptr::null();
    let mut len: usize = 0;

    let fstype_name = CString::new("TYPE")?;

    unsafe {
        blkid_probe_lookup_value(probe, fstype_name.as_ptr(), &mut fstype, &mut len);
    };
    ensure!(!fstype.is_null(), "blkid failed to get fstype");
    let fstype = unsafe { CStr::from_ptr(fstype).to_string_lossy().into_owned() };
    unsafe {
        blkid_free_probe(probe);
    }
    Ok(fstype)
}

#[cfg(test)]
mod tests {
    use metalos_macros::test;
    use metalos_macros::vmtest;

    use super::evaluate_spec;
    use super::probe_fstype;

    #[test]
    fn test_evaluate_full_path() {
        let device = evaluate_spec("/dev/path_looking_disk_does_not_need_to_be_checked");
        assert_eq!(
            device,
            Some("/dev/path_looking_disk_does_not_need_to_be_checked".into())
        );
    }

    #[vmtest]
    fn test_evaluate_label() {
        let device = evaluate_spec("LABEL=/");
        assert_eq!(device, Some("/dev/vda".into()));
    }

    #[vmtest]
    fn test_probe_fstype() {
        let fstype = probe_fstype("/dev/vda").unwrap();
        assert_eq!(fstype, "btrfs");
    }
}
