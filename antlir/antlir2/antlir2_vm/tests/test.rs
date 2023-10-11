/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::Path;

#[link(name = "c")]
extern "C" {
    fn geteuid() -> u32;
}

#[test]
fn is_root() {
    assert_eq!(0, unsafe { geteuid() });
}

#[test]
fn env_propagated() {
    assert_eq!("1", std::env::var("ANTLIR2_TEST").expect("env var missing"));
}

#[test]
fn env_artifact_exists() {
    let artifact = std::env::var("ENV_ARTIFACT");
    assert!(artifact.is_ok());
    assert!(Path::new(&artifact.expect("Already checked")).exists());
}

#[test]
fn no_tpm() {
    assert!(!Path::new("/dev/tpm0").exists());
    assert!(!Path::new("/dev/tpmrm0").exists());
}
