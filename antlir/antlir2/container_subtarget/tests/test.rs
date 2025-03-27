/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use rexpect::process::wait::WaitStatus;
use rexpect::session::PtySession;

const TIMEOUT_MS: Option<u64> = Some(60_000);

fn test_base(exe: &str) -> PtySession {
    let mut p = rexpect::spawn(exe, TIMEOUT_MS).expect("failed to spawn");
    // it would be better to set an explicit bash prompt, but that's hard to do
    // without an rcfile, so we just rely on it having a '# '
    p.exp_regex("[^\n](.*?)# ").expect("didn't get bash prompt");
    p.send_line("echo 'testing that the shell works'")
        .expect("failed to write shell command line");
    p.exp_string("testing that the shell works")
        .expect("didn't get echo output");
    p
}

fn test_exit_code(exe: &str, code: i32) {
    let mut p = test_base(exe);
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
fn test_rooted_simple() {
    let exe = std::env::var("ROOTED").expect("missing env var");
    test_exit_code(&exe, 0);
}

#[test]
fn test_rooted_exit_code() {
    let exe = std::env::var("ROOTED").expect("missing env var");
    test_exit_code(&exe, 42);
}

#[test]
fn test_rootless_simple() {
    let exe = std::env::var("ROOTLESS").expect("missing env var");
    test_exit_code(&exe, 0);
}

#[test]
fn test_rootless_exit_code() {
    let exe = std::env::var("ROOTLESS").expect("missing env var");
    test_exit_code(&exe, 42);
}

#[test]
fn test_rooted_boot_exit_code() {
    let exe = std::env::var("ROOTED").expect("missing env var");
    let mut p = rexpect::spawn(&format!("{exe} --boot --no-register"), TIMEOUT_MS)
        .expect("failed to spawn");
    p.exp_regex("[^\n\r](.*?)# ")
        .expect("didn't get bash prompt");
    p.send_line("exit 42")
        .expect("failed to write shell command line");
    let status = p.process.wait().expect("failed to wait for process");
    match status {
        WaitStatus::Exited(_, real_code) => assert_eq!(42, real_code),
        status => panic!("unexpected exit status: {status:?}"),
    }
}
