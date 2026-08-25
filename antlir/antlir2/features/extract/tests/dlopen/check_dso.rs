/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashSet;
use std::path::Path;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Expectation {
    #[serde(default)]
    present: Vec<String>,
    #[serde(default)]
    absent: Vec<String>,
}

fn try_unmount() {
    let output = std::process::Command::new("umount")
        .arg("/usr/local/fbcode")
        .output()
        .expect("failed to spawn umount");
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("not mounted") || stderr.contains("no mount point") {
        } else {
            let lazy = std::process::Command::new("umount")
                .args(["-l", "/usr/local/fbcode"])
                .output()
                .expect("failed to spawn umount -l");
            assert!(
                lazy.status.success(),
                "umount /usr/local/fbcode failed: {:?} stderr: {} and lazy also failed: {:?} stderr: {}",
                output.status.code(),
                stderr,
                lazy.status.code(),
                String::from_utf8_lossy(&lazy.stderr)
            );
        }
    }
    let _ = std::process::Command::new("umount")
        .arg("/mnt/gvfs")
        .output();
}

fn collect_filenames_under(root: &Path) -> HashSet<String> {
    let mut names = HashSet::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if let Ok(ft) = entry.file_type() {
                if ft.is_dir() {
                    stack.push(path);
                } else if ft.is_file() || ft.is_symlink() {
                    if let Some(fname) = path.file_name().and_then(|n| n.to_str()) {
                        names.insert(fname.to_string());
                    }
                }
            }
        }
    }
    names
}

fn matches_expected(found: &str, expected: &str) -> bool {
    found == expected || found.starts_with(&format!("{}.", expected))
}

#[test]
fn check_dso() {
    try_unmount();

    let json_str = std::env::var("DSO_CHECK").expect(
        "DSO_CHECK env var must be set to JSON like {\"present\": [\"libz.so\"], \"absent\": [\"liblzma.so\"]}",
    );
    let exp: Expectation = serde_json::from_str(&json_str)
        .expect("DSO_CHECK should be valid JSON with present/absent arrays");

    let found = collect_filenames_under(Path::new("/usr/local/fbcode"));

    for lib in &exp.present {
        if !found.iter().any(|f| matches_expected(f, lib)) {
            eprintln!("found files: {:?}", found);
            panic!(
                "expected {} to be present (any version) but it was absent. DSO_CHECK={}",
                lib, json_str
            );
        } else {
            println!("found {} (expected present, any version)", lib);
        }
    }

    for lib in &exp.absent {
        if found.iter().any(|f| matches_expected(f, lib)) {
            eprintln!("found files: {:?}", found);
            panic!(
                "expected {} to be absent (any version) but it was present. DSO_CHECK={}",
                lib, json_str
            );
        } else {
            println!("{} correctly absent (any version)", lib);
        }
    }

    println!("PASS");
}
