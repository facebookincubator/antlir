/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashSet;
use std::fmt::Display;
use std::process::Command;
use std::str::FromStr;

static LEVELS: &[&str] = &["root", "intermediate", "leaf"];

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum TestOs {
    Centos(u32),
    Eln,
}

impl FromStr for TestOs {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Some(num) = s.strip_prefix("centos") {
            Ok(Self::Centos(num.parse().map_err(|e| {
                format!("invalid number after centos: {}", e)
            })?))
        } else if s == "eln" {
            Ok(Self::Eln)
        } else {
            Err(format!("unsupported OS: {s}"))
        }
    }
}

impl Display for TestOs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TestOs::Centos(v) => write!(f, "centos{}", v),
            TestOs::Eln => write!(f, "eln"),
        }
    }
}

fn os_from_env() -> TestOs {
    std::env::var("OS")
        .expect("OS env var missing")
        .parse()
        .expect("invalid OS env var")
}

#[test]
fn os_select() {
    let os = os_from_env().to_string();
    for level in LEVELS {
        let filename = format!("/{level}.os");
        assert_eq!(
            os,
            std::fs::read_to_string(&filename)
                .unwrap_or_else(|_| panic!("failed to read {filename}")),
        );
    }
}

#[test]
fn os_release_rpm() {
    let os = os_from_env();
    let os_release_package = match os {
        TestOs::Centos(_) => "centos-stream-release",
        TestOs::Eln => "fedora-release-eln",
    };
    let proc = Command::new("rpm")
        .arg("-q")
        .arg(os_release_package)
        .arg("--queryformat=%{RPMTAG_VERSION}\n")
        .output()
        .expect("failed to run rpm");
    assert!(proc.status.success(), "rpm command failed");
    let ver = std::str::from_utf8(&proc.stdout).expect("rpm stdout not utf8");
    match os {
        TestOs::Centos(num) => {
            let ver_major = ver
                .split('.')
                .next()
                .expect("invalid centos version format");
            assert_eq!(num.to_string(), ver_major);
        }
        TestOs::Eln => {
            assert!(!ver.is_empty());
        }
    }
}

#[test]
fn synthetic_rpm() {
    let os = os_from_env();
    let proc = Command::new("rpm")
        .arg("-q")
        .arg("--queryformat=%{RPMTAG_VERSION}\n")
        .args(LEVELS.iter().map(|l| format!("test-rpm-{l}")))
        .output()
        .expect("failed to run rpm");
    assert!(proc.status.success(), "rpm command failed");
    let releases: HashSet<&str> = std::str::from_utf8(&proc.stdout)
        .expect("rpm stdout not utf8")
        .lines()
        .collect();
    let os_str = os.to_string();
    assert_eq!(HashSet::from([os_str.as_str()]), releases);
}
