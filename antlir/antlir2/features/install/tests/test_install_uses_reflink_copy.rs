/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fs::File;
use std::io::Result;
use std::os::unix::io::AsRawFd;
use std::path::Path;

use nix::libc::c_ulong;

#[test]
fn test_file_shares_extents() {
    let image_file_path = Path::new("/installed");
    let image_extents = fiemap(
        File::open(image_file_path)
            .unwrap_or_else(|_| panic!("failed to open {}", image_file_path.display())),
    )
    .expect("fiemap failed");
    eprintln!("{image_extents:#?}");
    assert!(
        image_extents
            .iter()
            .all(|e| e.fe_flags.contains(ExtentFlags::SHARED)),
        "all extents should have SHARED flag set"
    );
    // we could check the extents of the source file too, but it might not
    // actually be the same exact file because of how buck platform
    // configurations work, so we can be satisfied that all the extents have the
    // SHARED flag on them
}

const FS_IOC_FIEMAP: c_ulong = 0xC020660B;
const PAGESIZE: usize = 8;

#[derive(Copy, Clone, Debug)]
#[repr(C)]
pub struct Req {
    fm_start: u64,
    fm_length: u64,
    fm_flags: u32,
    fm_mapped_extents: u32,
    fm_extent_count: u32,
    fm_reserved: u32,
    fm_extents: [Extent; PAGESIZE],
}

impl Default for Req {
    fn default() -> Self {
        Self {
            fm_start: 0,
            fm_length: u64::max_value(),
            fm_flags: 0,
            fm_mapped_extents: 0,
            fm_extent_count: PAGESIZE as u32,
            fm_reserved: 0,
            fm_extents: [Default::default(); PAGESIZE],
        }
    }
}

#[repr(C)]
#[derive(Default, Copy, Clone)]
struct Extent {
    fe_logical: u64,
    fe_physical: u64,
    fe_length: u64,
    fe_reserved64: [u64; 2],
    fe_flags: ExtentFlags,
    fe_reserved: [u32; 3],
}

impl std::fmt::Debug for Extent {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.debug_struct("Extent")
            .field("fe_logical", &self.fe_logical)
            .field("fe_physical", &self.fe_physical)
            .field("fe_length", &self.fe_length)
            .field("fe_flags", &self.fe_flags)
            .finish()
    }
}

bitflags::bitflags! {
    #[derive(Default, Copy, Clone, Debug)]
    struct ExtentFlags: u32 {
      /// This is generally the last extent in the file. A mapping attempt
      /// past this extent may return nothing. Some implementations set this
      /// flag to indicate this extent is the last one in the range queried
      /// by the user (via fiemap->fm_length).
      const LAST           = 0x00000001;
      /// The location of this extent is currently unknown. This may
      /// indicate the data is stored on an inaccessible volume or that no
      /// storage has been allocated for the file yet.
      const UNKNOWN        = 0x00000002;
      /// This will also set FIEMAP_EXTENT_UNKNOWN
      /// Delayed allocation - while there is data for this extent, its
      /// physical location has not been allocated yet.
      const DELALLOC       = 0x00000004;
      /// This extent does not consist of plain filesystem blocks but is
      /// encoded (e.g. encrypted or compressed). Reading the data in this
      /// extent via I/O to the block device will have undefined results.
      const ENCODED        = 0x00000008;
      /// This will also set FIEMAP_EXTENT_ENCODED
      /// The data in this extent has been encrypted by the file system.
      const DATA_ENCRYPTED = 0x00000080;
      /// Extent offsets and length are not guaranteed to be block aligned.
      const NOT_ALIGNED    = 0x00000100;
      /// This will also set FIEMAP_EXTENT_NOT_ALIGNED
      /// Data is located within a meta data block.
      const DATA_INLINE    = 0x00000200;
      /// This will also set FIEMAP_EXTENT_NOT_ALIGNED
      /// Data is packed into a block with data from other files.
      const DATA_TAIL      = 0x00000400;
      /// Unwritten extent - the extent is allocated but its data has not
      /// been initialized. This indicates the extent's data will be all
      /// zero if read through the filesystem but the contents are undefined
      /// if read directly from the device.
      const UNWRITTEN      = 0x00000800;
      /// This will be set when a file does not support extents, i.e., it
      /// uses a block based addressing scheme. Since returning an extent
      /// for each block back to userspace would be highly inefficient, the
      /// kernel will try to merge most adjacent blocks into 'extents'.
      const MERGED         = 0x00001000;
      /// Extent is shared between multiple files.
      const SHARED         = 0x00002000;
    }
}

nix::ioctl_readwrite_bad!(ioctl_fiemap, FS_IOC_FIEMAP, Req);

fn fiemap<F: AsRawFd>(fd: F) -> Result<Vec<Extent>> {
    let mut results = Vec::new();
    let mut req = Req::default();
    'getall: loop {
        unsafe { ioctl_fiemap(fd.as_raw_fd(), &mut req) }?;
        for extent in req.fm_extents.iter().take(req.fm_mapped_extents as usize) {
            results.push(extent.clone());
            if extent.fe_flags.contains(ExtentFlags::LAST) {
                break 'getall;
            }
        }
        if req.fm_mapped_extents == 0 {
            break;
        }
        let last = results.last().expect("would have broken already");
        req.fm_start = last.fe_logical + last.fe_length;
    }
    Ok(results)
}
