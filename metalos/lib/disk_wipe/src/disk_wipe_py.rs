use disk_wipe::quick_wipe_disk as quick_wipe_disk_rs;
use metalos_disk::DiskDevPath;
extern crate cpython;
use cpython::*;

py_module_initializer!(
    disk_wipe_py,
    initdisk_wipe_py,
    PyInit_disk_wipe_py,
    |py, m| {
        m.add(
            py,
            "quick_wipe_disk",
            py_fn!(py, quick_wipe_disk(block_device_str: String)),
        )?;
        Ok(())
    }
);

fn quick_wipe_disk(py: Python, block_device_str: String) -> PyResult<PyNone> {
    let mut disk = DiskDevPath(block_device_str.into());
    quick_wipe_disk_rs(&mut disk)
        .map_err(|e| PyErr::new::<exc::Exception, _>(py, format!("{:#}", e)))?;
    Ok(PyNone)
}
