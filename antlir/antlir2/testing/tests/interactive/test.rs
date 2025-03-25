/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use rexpect::process::wait::WaitStatus;
use rexpect::session::PtySession;

fn booted_test_base(exe: &str) -> PtySession {
    let mut p = rexpect::spawn(exe, Some(5_000)).expect("failed to spawn");
    // look for the booted test debugging help message
    p.exp_string("This is an antlir2 booted image test.")
        .expect("missing help message");
    // run something in the shell
    p.send_line("echo 'testing that the shell works'")
        .expect("failed to write shell command line");
    p.exp_string("testing that the shell works")
        .expect("didn't get echo output");
    // actually run the test
    p.send_line("/__antlir2_image_test__/image-test exec")
        .expect("failed to write shell command line");
    p.exp_string("This is the output from tests of the interactive debug console")
        .expect("didn't get echo output");
    p
}

fn booted_test_exit_code(exe: &str, code: i32) {
    let mut p = booted_test_base(exe);
    p.send_line(&format!("exit {code}"))
        .expect("failed to send exit command");
    p.exp_eof().expect("didn't get EOF");
    let status = p.process.wait().expect("failed to wait for process");
    match status {
        WaitStatus::Exited(_, real_code) => assert_eq!(code, real_code),
        status => panic!("unexpected exit status: {status:?}"),
    }
}

#[test]
fn booted_test_rooted_simple() {
    let exe = std::env::var("ECHO_ROOTED").expect("missing env var");
    booted_test_exit_code(&exe, 0);
}

#[test]
fn booted_test_rooted_exit_code() {
    let exe = std::env::var("ECHO_ROOTED").expect("missing env var");
    booted_test_exit_code(&exe, 42);
}

// TODO: until D70912826, these "rootless" tests are technically a lie since
// `rootless=True` is ignored, but it's useful to test now to make sure the
// [container] subtarget doesn't regress when it does start respecting
// rootless=True
#[test]
fn booted_test_rootless_simple() {
    let exe = std::env::var("ECHO_ROOTLESS").expect("missing env var");
    booted_test_exit_code(&exe, 0);
}

#[test]
fn booted_test_rootless_exit_code() {
    let exe = std::env::var("ECHO_ROOTLESS").expect("missing env var");
    booted_test_exit_code(&exe, 42);
}
