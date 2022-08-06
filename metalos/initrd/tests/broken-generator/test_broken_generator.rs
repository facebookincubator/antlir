use std::path::Path;
use std::process::Command;

use anyhow::Result;
use systemd::Systemd;

/// The metalos generator is responsible for setting the default target back to
/// initrd.target, since this test replaces it with a broken mocked version it
/// should still be emergency.target
#[test]
fn emergency_target_is_default() -> Result<()> {
    let out = Command::new("systemctl")
        .arg("get-default")
        .arg("--root=/")
        .output()?;
    assert_eq!(Ok("emergency.target\n"), std::str::from_utf8(&out.stdout));
    Ok(())
}

/// This test layer has a dropin to get emergency.service to write out a
/// sentinel file that is checked in this test. Since systemd is intentionally
/// in a state where almost everything is disabled (including dbus), this is the
/// best way to verify.
#[test]
fn in_emergency_target() {
    assert!(
        Path::new("/run/I_AM_IN_EMERGENCY").exists(),
        "emergency.service doesn't appear to have been run"
    );
}

/// DBus will not be started in emergency.target, so make sure that we can't
/// connect to Systemd (the library will automatically retry, so this can't pass
/// due to a simple race condition)
#[tokio::test]
async fn dbus_is_unavailable() -> Result<()> {
    let log = slog::Logger::root(slog_glog_fmt::default_drain(), slog::o!());
    let err = Systemd::connect(log).await.map(|_| ()).unwrap_err();
    match err {
        systemd::Error::Connect(_) => {}
        _ => panic!("expected Connect error, not {:?}", err),
    };
    Ok(())
}
