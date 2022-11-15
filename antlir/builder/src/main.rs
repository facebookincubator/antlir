/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::ffi::OsString;
use std::io::Write as _;
use std::os::unix::fs::DirBuilderExt;
use std::path::PathBuf;
use std::process::Command;

use anyhow::ensure;
use anyhow::Context;
use anyhow::Result;
use buck_label::Label;
use clap::Parser;
use clap::ValueEnum;
use fs2::FileExt;
use slog::info;
use slog::Drain;
use walkdir::WalkDir;

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum)]
enum BuckVersion {
    #[clap(name = "1")]
    One,
    #[clap(name = "2")]
    Two,
}

impl BuckVersion {
    fn buck_cmd(self) -> &'static str {
        match self {
            Self::One => "buck",
            Self::Two => "buck2",
        }
    }
}

#[derive(Parser)]
struct Args {
    #[clap(long, value_enum)]
    buck_version: BuckVersion,
    #[clap(long, env = "ANTLIR_DEBUG", help = "be extra verbose")]
    debug: bool,
    #[clap(long)]
    label: Label,
    #[clap(long)]
    tmp_dir: PathBuf,
    #[clap(long)]
    out: PathBuf,
    #[allow(unused)]
    #[clap(long, help = "unused arg to force buck cache-busting")]
    cache_buster: Option<String>,
    #[clap(flatten)]
    tools: Tools,
    #[clap(help = "binary to execute after prepping buck-image-out")]
    wrapped_cmd: PathBuf,
    #[clap(help = "arguments to pass to wrapped binary")]
    wrapped_args: Vec<OsString>,
}

#[derive(Parser)]
struct Tools {
    // Tools below here are targeted to be absorbed into this binary or be made
    // unnecessary with buck2 providers
    #[clap(long)]
    ensure_artifacts_dir_exists: PathBuf,
    #[clap(long)]
    volume_for_repo: PathBuf,
}

#[derive(Debug, Clone)]
struct WrappedEnv {
    subvolumes_dir: PathBuf,
}

impl IntoIterator for WrappedEnv {
    type Item = (OsString, OsString);
    type IntoIter = <Vec<(OsString, OsString)> as IntoIterator>::IntoIter;

    #[deny(unused_variables)]
    fn into_iter(self) -> Self::IntoIter {
        let WrappedEnv { subvolumes_dir } = self;
        vec![("SUBVOLUMES_DIR".into(), subvolumes_dir.into())].into_iter()
    }
}

fn main() -> Result<()> {
    let args = Args::parse();
    let filter_level = match args.debug {
        true => slog::Level::Warning,
        false => slog::Level::Debug,
    };
    let log = slog::Logger::root(
        slog_glog_fmt::default_drain()
            .filter_level(filter_level)
            .fuse(),
        slog::o!(
            "label" => args.label.to_string(),
        ),
    );
    let start_time = chrono::Local::now();
    let start_instant = std::time::Instant::now();

    // TODO(image_out): add initial creation functionality to `image_out` lib to
    // remove the need to shell out to python
    let out = Command::new(&args.tools.ensure_artifacts_dir_exists)
        .env("ANTLIR_BUCK", args.buck_version.buck_cmd())
        .output()
        .context("while running ensure_artifacts_dir_exists")?;
    ensure!(
        out.status.success(),
        "ensure_artifacts_dir_exists failed: {}",
        std::str::from_utf8(&out.stderr).unwrap_or("<not utf8>")
    );
    let artifacts_dir: PathBuf = std::str::from_utf8(&out.stdout)
        .context("ensure_artifacts_dir_exists not utf8")?
        .trim()
        .into();
    info!(log, "found artifacts dir: {}", artifacts_dir.display());
    let out = Command::new(&args.tools.volume_for_repo)
        .arg(&artifacts_dir)
        .output()
        .context("while running volume_for_repo")?;
    ensure!(
        out.status.success(),
        "volume_for_repo failed: {}",
        std::str::from_utf8(&out.stderr).unwrap_or("<not utf8>")
    );
    let volume_for_repo: PathBuf = std::str::from_utf8(&out.stdout)
        .context("volume_for_repo not utf8")?
        .trim()
        .into();
    info!(log, "found volume: {}", volume_for_repo.display());
    let subvolumes_dir = volume_for_repo.join("targets");
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&subvolumes_dir)
        .with_context(|| format!("while creating '{}'", subvolumes_dir.display()))?;

    let wrapped_env = WrappedEnv { subvolumes_dir };

    let wrapped_out = Command::new(&args.wrapped_cmd)
        .args(args.wrapped_args)
        .envs(wrapped_env)
        .output()
        .with_context(|| format!("while running {}", args.wrapped_cmd.display()))?;
    let stdout = strip_ansi_escapes::strip(&wrapped_out.stdout)
        .context("while stripping ansi escapes from stdout")?;
    let stderr = strip_ansi_escapes::strip(&wrapped_out.stderr)
        .context("while stripping ansi escapes from stderr")?;
    ensure!(
        wrapped_out.status.success(),
        "wrapped command failed: stderr: {}",
        std::str::from_utf8(&stderr).unwrap_or("<not utf8>")
    );

    let elapsed = chrono::Duration::from_std(std::time::Instant::elapsed(&start_instant))
        .expect("if image build times overflow, we have bigger problems");

    let mut logs_file = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(artifacts_dir.join("image_build.log"))
        .context("while opening logs file")?;
    logs_file
        .lock_exclusive()
        .context("while locking logs file")?;
    writeln!(
        logs_file,
        "{} {} (elapsed {})",
        start_time.to_rfc3339(),
        args.label,
        elapsed,
    )
    .context("while writing header to logs file")?;
    logs_file
        .write_all(&stdout)
        .context("while copying logs to logs file")?;
    logs_file
        .write_all(&stderr)
        .context("while copying logs to logs file")?;

    // It is always a terrible idea to mutate Buck outputs after creation, so as
    // an extra safety precation let's mark them readonly so we can't
    // accidentally do anything stupid later.
    for entry in WalkDir::new(&args.out) {
        let entry = entry.with_context(|| format!("while walking '{}'", args.out.display()))?;
        // Buck cleanup can't handle readonly directories
        if !entry.file_type().is_dir() {
            let mut perms = entry
                .metadata()
                .with_context(|| format!("while statting '{}'", entry.path().display()))?
                .permissions();
            perms.set_readonly(true);
            std::fs::set_permissions(entry.path(), perms)
                .with_context(|| format!("while making '{}' RO", entry.path().display()))?;
        }
    }

    Ok(())
}
