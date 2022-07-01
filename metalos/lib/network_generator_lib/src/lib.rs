use std::fs;
use std::io::BufWriter;
use std::os::unix::fs::symlink;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use slog::info;
use slog::o;
use slog::Drain;
use slog::Logger;
use slog_glog_fmt::kv_categorizer::ErrorCategorizer;
use structopt::StructOpt;

use systemd::render::NetworkUnit;
use systemd::render::NetworkUnitMatchSection;
use systemd_generator_lib::write_dropin_to_disk;
use systemd_generator_lib::Dropin;
use systemd_generator_lib::GeneratorArgs;

// WARNING: keep in sync with the bzl/TARGETS file unit
const ETH_NETWORK_UNIT_FILENAME: &str = "50-eth.network";

#[derive(Debug, StructOpt)]
#[cfg_attr(test, derive(Clone))]
pub struct Args {
    #[structopt(flatten)]
    generator_args: GeneratorArgs,

    /// What directory to place the network unit dropin/override in
    #[structopt(default_value = "/usr/lib/systemd/network/")]
    network_unit_dir: PathBuf,

    /// Where to find the default target on success
    #[structopt(long, default_value = "/usr/lib/systemd/system/initrd.target")]
    success_default_target: PathBuf,
}

fn update_default_target(args: Args) -> Result<()> {
    let default_target_path = args.generator_args.early_dir.join("default.target");
    match std::fs::remove_file(&default_target_path) {
        // NotFound error is Ok, others are not
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        x => x,
    }
    .context("while removing original default.target")?;
    symlink(args.success_default_target, default_target_path)
        .context("while changing default target to initrd.target")?;

    Ok(())
}

/// This is the main logic of the code and it's in its own function so that we can unit test
/// and pass in mocked out args and logger
fn run_generator<F>(log: Logger, args: Args, get_mac_address: F) -> Result<()>
where
    F: FnOnce() -> Result<String>,
{
    let dropin = Dropin {
        target: ETH_NETWORK_UNIT_FILENAME.into(),
        unit: NetworkUnit {
            match_section: NetworkUnitMatchSection {
                name: "eth*".to_string(),
                mac_address: get_mac_address().context("failed to get mac address")?,
            },
        },
        dropin_filename: Some("match.conf".to_string()),
    };

    write_dropin_to_disk(
        &dropin,
        log.clone(),
        &args.network_unit_dir,
        Path::new("match.conf"),
    )
    .context("failed to write dropin to disk")?;

    // IMPORTANT: this MUST be the last thing that the generator does, otherwise
    // any bugs in the generator can be masked and cause future hard-to-diagnose
    // failures
    update_default_target(args)
        .context("failed to update default target after successful generator run")?;
    info!(
        log,
        "successfully updated default target after successful generator run"
    );
    Ok(())
}

pub fn setup_kmsg_logger() -> Result<Logger> {
    // generators have an additional logging drain setup that is not as
    // pretty looking as other slog drain formats, but is usable with /dev/kmsg.
    // Otherwise, the regular drain that logs to stderr silently disappears when
    // systemd runs the generator.
    let log = slog::Logger::root(slog_glog_fmt::default_drain(), o!());
    let kmsg = fs::OpenOptions::new()
        .write(true)
        .open("/dev/kmsg")
        .context("failed to open /dev/kmsg for logging")?;
    let kmsg = BufWriter::new(kmsg);

    let decorator = slog_term::PlainDecorator::new(kmsg);
    let drain = slog_glog_fmt::GlogFormat::new(decorator, ErrorCategorizer).fuse();
    let drain = slog_async::Async::new(drain).build().fuse();

    Ok(slog::Logger::root(
        slog::Duplicate::new(log, drain).fuse(),
        o!(),
    ))
}

pub fn generator_main<F>(get_mac_address: F) -> Result<()>
where
    F: FnOnce() -> Result<String>,
{
    let log = setup_kmsg_logger().context("failed to setup kmsg logger")?;
    let args = Args::from_args();
    run_generator(log, args, get_mac_address)
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::BTreeMap;

    use anyhow::anyhow;
    use anyhow::bail;
    use maplit::btreemap;

    fn setup_generator_test(name: &'static str) -> Result<(Logger, PathBuf, Args)> {
        let log = slog::Logger::root(slog_glog_fmt::default_drain(), o!());

        let (tmpdir, generator_args) = systemd_generator_lib::setup_generator_test(name)?;

        let network = tmpdir.join("network");
        std::fs::create_dir(&network).context("failed to create network dir")?;

        let success_target = tmpdir.join("success.initrd");
        std::fs::File::create(&success_target)
            .context("failed to create fake success target file")?;

        let args = Args {
            generator_args,
            network_unit_dir: network,
            success_default_target: success_target,
        };

        Ok((log, tmpdir, args))
    }

    #[derive(Debug)]
    enum GeneratedFile {
        Contents(String),
        SymlinkTo(PathBuf),
    }

    impl From<String> for GeneratedFile {
        fn from(s: String) -> Self {
            Self::Contents(s)
        }
    }

    impl From<&str> for GeneratedFile {
        fn from(s: &str) -> Self {
            s.to_string().into()
        }
    }

    impl From<PathBuf> for GeneratedFile {
        fn from(p: PathBuf) -> Self {
            Self::SymlinkTo(p)
        }
    }

    fn compare_dir_inner(
        base_dir: &Path,
        expected_contents: &mut BTreeMap<PathBuf, GeneratedFile>,
    ) -> Result<()> {
        for entry in std::fs::read_dir(base_dir).context("failed to read base dir")? {
            let entry = entry.context("failed to read next entry from base dir")?;
            let path = entry.path();
            if path.is_dir() {
                compare_dir_inner(&path, expected_contents)
                    .context(format!("Failed to process directory {:?}", path))?;
            } else {
                match expected_contents.remove(&path) {
                    Some(expected_content) => match expected_content {
                        GeneratedFile::Contents(expected_content) => {
                            let content = std::fs::read_to_string(&path)
                                .context(format!("Can't read file {:?}", path))?;

                            if expected_content != content {
                                return Err(anyhow!(
                                    "File contents for {:?} differs from expected:\ncontents: {:?}\nexpected: {:?}\n",
                                    path,
                                    content,
                                    expected_content,
                                ));
                            }
                        }
                        GeneratedFile::SymlinkTo(dst) => {
                            match std::fs::read_link(&path) {
                                Ok(link_dst) => {
                                    if dst != link_dst {
                                        bail!(
                                            "Expected {:?} to link to {:?}, but actually pointed to {:?}",
                                            path,
                                            dst,
                                            link_dst
                                        );
                                    }
                                }
                                Err(e) => bail!(
                                    "Expected {:?} to link to {:?}, but reading the link failed: {:?}",
                                    path,
                                    dst,
                                    e
                                ),
                            };
                        }
                    },
                    None => {
                        return Err(anyhow!(
                            "Found unexpected file {:?} in directory {:?}",
                            entry.path(),
                            base_dir
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    fn compare_dir(
        base_dir: &Path,
        mut expected_contents: BTreeMap<PathBuf, GeneratedFile>,
    ) -> Result<()> {
        compare_dir_inner(base_dir, &mut expected_contents)?;
        if expected_contents.is_empty() {
            Ok(())
        } else {
            let keys: Vec<PathBuf> = expected_contents.into_iter().map(|(k, _)| k).collect();
            Err(anyhow!(
                "At least one file not found in {:?}: {:?}",
                base_dir,
                keys
            ))
        }
    }

    #[test]
    fn test_basic_success() -> Result<()> {
        let (log, tmpdir, args) =
            setup_generator_test("basic_success").context("failed to setup test")?;

        run_generator(log, args.clone(), || Ok("11:22:33:44:55:66".to_string()))
            .context("failed to run generator")?;

        compare_dir(
            &tmpdir,
            btreemap! {
                args.network_unit_dir.join("50-eth.network.d/match.conf") => "\
                    [Match]\n\
                    Name=eth*\n\
                    MACAddress=11:22:33:44:55:66\n\
                    ".into(),
                args.generator_args.early_dir.join("default.target") => args.success_default_target.clone().into(),
                args.success_default_target => "".into(),
            },
        )
        .context("Failed to ensure tmpdir is setup correctly")
    }
}
