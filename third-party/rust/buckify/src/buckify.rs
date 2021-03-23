use std::env;

use anyhow::{Context, Result};
use cargo_lock::Lockfile;

fn main() -> Result<()> {
    let args: Vec<_> = env::args().collect();
    let lockfile = Lockfile::load(&args[1])?;

    let mut pkgs = lockfile.packages;
    pkgs.sort_by_key(|pkg| pkg.name.to_string());

    println!("load(\":defs.bzl\", \"third_party_rust_library\")");

    for pkg in &pkgs {
        if pkg.name.to_string() == "antlir-deps" {
            continue;
        }
        println!("third_party_rust_library(");
        println!("  name = \"{}\",", pkg.name);
        println!("  version = \"{}\",", pkg.version);
        let checksum = pkg
            .checksum
            .as_ref()
            .with_context(|| format!("{} missing checksum", pkg.name))?;
        assert!(
            checksum.is_sha256(),
            format!("{} checksum is not sha256", pkg.name)
        );
        println!("  sha256 = \"{}\",", checksum);
        println!(
            "  deps = [{}]",
            pkg.dependencies
                .iter()
                .map(|dep| format!("\":{}\"", dep.name))
                .collect::<Vec<_>>()
                .join(", ")
        );
        println!(")");
    }
    Ok(())
}
