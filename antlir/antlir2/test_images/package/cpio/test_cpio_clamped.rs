/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fs::File;
use std::io::BufReader;
use std::io::Read;

/// (name, mtime, ino) of each entry
fn entries(resource: &str) -> Vec<(String, u32, u32)> {
    let path = buck_resources::get(format!(
        "antlir/antlir2/test_images/package/cpio/{resource}"
    ))
    .expect("failed to get cpio resource");
    let mut input = BufReader::new(File::open(path).expect("failed to open cpio"));
    let mut out = Vec::new();
    loop {
        let mut reader = cpio::NewcReader::new(input).expect("failed to read cpio entry");
        if reader.entry().is_trailer() {
            break;
        }
        let entry = reader.entry();
        out.push((entry.name().to_owned(), entry.mtime(), entry.ino()));
        std::io::copy(&mut reader.by_ref(), &mut std::io::sink()).expect("failed to read data");
        input = reader.finish().expect("failed to finish entry");
    }
    out
}

#[test]
fn clamped() {
    let clamped = entries("test-clamped.cpio");
    let unclamped = entries("test.cpio");
    assert_eq!(
        clamped.iter().map(|e| &e.0).collect::<Vec<_>>(),
        unclamped.iter().map(|e| &e.0).collect::<Vec<_>>(),
        "clamping keeps the same entries",
    );
    assert!(
        unclamped.iter().any(|e| e.1 > 1000),
        "the layer has entries newer than the epoch to clamp",
    );
    for (name, mtime, _) in &clamped {
        assert!(
            *mtime <= 1000,
            "{name} has mtime {mtime}, newer than the epoch"
        );
    }
    // cpio --reproducible numbers inodes from 1 in archive order
    let first = clamped.first().expect("archive has entries");
    assert!(
        first.2 < 1000,
        "{} has inode {}, not renumbered",
        first.0,
        first.2
    );
}
