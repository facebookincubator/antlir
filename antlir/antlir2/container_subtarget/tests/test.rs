/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::env;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

use anyhow::Result;
use rexpect::process::wait::WaitStatus;
use rexpect::session::PtySession;
use tempfile::TempDir;

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
    p.send_line("systemctl is-system-running")
        .expect("failed to write shell command line");
    p.exp_regex("running")
        .expect("didn't get 'running' response from systemctl is-system-running");
    p.send_line("exit 42")
        .expect("failed to write shell command line");
    let status = p.process.wait().expect("failed to wait for process");
    match status {
        WaitStatus::Exited(_, real_code) => assert_eq!(42, real_code),
        status => panic!("unexpected exit status: {status:?}"),
    }
}

#[test]
fn test_rootless_boot_exit_code() {
    let exe = std::env::var("ROOTLESS").expect("missing env var");
    let mut p = rexpect::spawn(&format!("{exe} --boot --no-register"), TIMEOUT_MS)
        .expect("failed to spawn");
    p.exp_regex("[^\n\r](.*?)# ")
        .expect("didn't get bash prompt");
    p.send_line("systemctl is-system-running")
        .expect("failed to write shell command line");
    p.exp_regex("running")
        .expect("didn't get 'running' response from systemctl is-system-running");
    p.send_line("exit 42")
        .expect("failed to write shell command line");
    let status = p.process.wait().expect("failed to wait for process");
    match status {
        WaitStatus::Exited(_, real_code) => assert_eq!(42, real_code),
        status => panic!("unexpected exit status: {status:?}"),
    }
}

fn relocated_runner() -> Result<(TempDir, Command)> {
    let dir = tempfile::tempdir_in("/tmp")?;
    let binary = dir.path().join("run");
    fs::copy(env::var("RUNNER")?, &binary)?;
    for name in ["sudo", "systemd-nspawn"] {
        let stub = dir.path().join(name);
        fs::write(&stub, "#!/bin/sh\nprintf '%s\\n' \"$@\"\nexit 42\n")?;
        fs::set_permissions(stub, fs::Permissions::from_mode(0o755))?;
    }
    let mut command = Command::new(binary);
    command
        .current_dir(dir.path())
        .env("PATH", dir.path())
        .env_remove("INSIDE_RE_WORKER")
        .args(["--subvol", "/rootfs"]);
    Ok((dir, command))
}

#[test]
fn test_relocated_without_repo() -> Result<()> {
    let (_dir, mut command) = relocated_runner()?;
    let output = command.output()?;
    assert_eq!(output.status.code(), Some(42), "{output:?}");
    let args = String::from_utf8(output.stdout)?;
    assert!(args.contains("--directory\n/rootfs\n"));
    assert!(args.ends_with("--\n/bin/bash\n"));
    assert!(!args.contains("--bind-ro\n"));
    Ok(())
}

#[test]
fn test_relocated_with_missing_repo() -> Result<()> {
    let (_dir, mut command) = relocated_runner()?;
    let output = command.arg("--artifacts-require-repo").output()?;
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(String::from_utf8(output.stderr)?.contains("while looking for repo root"));
    assert!(output.stdout.is_empty());
    Ok(())
}

#[test]
fn test_relocated_with_required_repo() -> Result<()> {
    let (dir, mut command) = relocated_runner()?;
    fs::create_dir(dir.path().join(".sl"))?;
    let output = command.arg("--artifacts-require-repo").output()?;
    assert_eq!(output.status.code(), Some(42), "{output:?}");
    let args = String::from_utf8(output.stdout)?;
    assert!(args.contains(&format!("--bind-ro\n{}\n", dir.path().display())));
    assert!(args.contains("--bind-ro\n/usr/local/fbcode\n"));
    Ok(())
}
