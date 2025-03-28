/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use bytesize::ByteSize;

fn size_of(resource: &str) -> ByteSize {
    let meta = buck_resources::get(format!(
        "antlir/antlir2/test_images/package/sendstream/incremental/{resource}"
    ))
    .expect("failed to get resource path")
    .metadata()
    .expect("failed to stat resource");
    ByteSize::b(meta.len())
}

macro_rules! assert_size_close {
    ($resource:literal, $expected:expr, $tolerance:expr) => {
        assert!(
            $tolerance < $expected,
            "tolerance should be smaller than the expected size"
        );
        let actual_size = size_of($resource);
        let min = ByteSize::b($expected.0.saturating_sub($tolerance.0));
        assert!(
            actual_size > min,
            "expected {} to be larger than {}, but it was {}",
            $resource,
            min,
            actual_size,
        );
        let max = $expected + $tolerance;
        assert!(
            actual_size < max,
            "expected {} to be smaller than {}, but it was {}",
            $resource,
            max,
            actual_size,
        );
    };
}

macro_rules! test_parent_child {
    ($parent:literal, $child:literal) => {
        assert_size_close!($parent, ByteSize::mb(256), ByteSize::mb(15));
        // the child has its own copy of the large 256mb random file, so it should
        // be about the same size, but it should *not* be 2x the size as it would be
        // if not incremental
        assert_size_close!($child, ByteSize::mb(256), ByteSize::mb(15));
    };
}

#[test]
fn test_incremental_size_rootless() {
    test_parent_child!("parent.sendstream.rootless", "child.sendstream.rootless");
}

#[test]
fn test_incremental_size_prebuilt() {
    test_parent_child!("prebuilt-parent.sendstream", "child-of-prebuilt.sendstream");
}

#[test]
fn test_incremental_size_prebuilt_rootless() {
    test_parent_child!(
        "prebuilt-parent.sendstream.rootless",
        "child-of-prebuilt.sendstream.rootless"
    );
}
