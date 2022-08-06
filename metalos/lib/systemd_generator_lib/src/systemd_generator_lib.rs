use std::collections::BTreeMap;
use std::io::Write;
use std::os::unix::fs::symlink;
use std::path::Path;
use std::path::PathBuf;
use std::time::SystemTime;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Result;
use serde::Serialize;
use slog::info;
use slog::Logger;
use structopt::StructOpt;
use systemd::render::MountSection;
use systemd::render::NetworkUnit;
use systemd::render::Render;
use systemd::render::Unit;
use systemd::render::UnitBody;
use systemd::render::UnitSection;
use systemd::UnitName;

pub const ENVIRONMENT_FILENAME: &str = "metalos_environment";

pub const ROOTDISK_MOUNT_SERVICE: &str = "run-fs-control.mount";

/// This represents some static set of known environment variables we want to build and put into
/// all units that want metalos info. You should add your variables names as
/// #[serde(rename = "MY_ENV_VAR")] and if you want to reuse sections of common variables you can
/// use #[serde(flatten)].
///
/// We expect every field in the struct to be converted by serde into a `serde_json::Value::String`
/// so that we can write it into the environment file without any conversions. If you need custom
/// logic you can use serdes attributes to do so or convert before assigning.
pub trait Environment: Serialize + Sized {
    fn write_systemd_env_file(self, base_dir: &Path, filename: &Path) -> Result<PathBuf> {
        let env_file_path = base_dir.join(filename);
        let file = std::fs::File::create(&env_file_path).context(format!(
            "Failed to create environment file {:?} in {:?}",
            filename, base_dir
        ))?;
        let mut file = std::io::LineWriter::new(file);

        let env_map = self
            .try_into_map()
            .context("failed to convert environment to map")?;

        for (k, v) in env_map {
            write!(&mut file, "{}={}\n", k, v).context("failed to write next line in file")?;
        }

        Ok(env_file_path)
    }

    fn try_into_map(self) -> Result<BTreeMap<String, String>> {
        let value =
            serde_json::to_value(self).context("failed to convert environment to json value")?;

        let map = match value {
            serde_json::Value::Object(map) => Ok(map),
            _ => Err(anyhow!("Got unexpected type {:?} from ", value)),
        }?;

        let mut out = BTreeMap::new();
        for (k, v) in map {
            let v_string = match v {
                serde_json::Value::Null => continue,
                serde_json::Value::String(s) => s,
                _ => {
                    return Err(anyhow!("Got unexpected value {:?} for key {}", v, k));
                }
            };
            out.insert(k, v_string);
        }
        Ok(out)
    }
}

/// This struct represents an extra dependency that we add between two units.
/// it says that `source` requires `requires`.
#[derive(Debug, PartialEq)]
pub struct ExtraDependency {
    pub source: UnitName,
    pub requires: UnitName,
}

/// A mapping of dependency name (must be valid as a filename) and a extra dependency
pub type ExtraDependencies = Vec<(String, ExtraDependency)>;

#[derive(Debug, PartialEq, PartialOrd)]
pub struct MountUnit {
    pub unit_section: UnitSection,
    pub mount_section: MountSection,
}

/// This represents a dropin unit that the generator can make. The unit inside can be
/// written to disk with the `systemd::render::Render` trait and we also hold a name
/// of the unit that we are targeting
#[derive(Debug, PartialEq)]
pub struct Dropin<UNIT: Render> {
    pub target: UnitName,
    pub unit: UNIT,
    // this allows to personalise the name of the drop-in file
    pub dropin_filename: Option<String>,
}

fn write_extra_dependency(
    log: Logger,
    normal_dir: &Path,
    name: &str,
    dep: ExtraDependency,
) -> Result<()> {
    write_dropin_to_disk(
        &Dropin {
            target: dep.source,
            unit: Unit {
                unit: Some(UnitSection {
                    requires: Some(dep.requires.clone()),
                    // We have a single field here for requires but it
                    // implies both dep and ordering because we need that
                    // in all cases so far.
                    after: Some(dep.requires),
                }),
                body: None,
            },
            dropin_filename: None,
        },
        log,
        normal_dir,
        Path::new(&format!("{}.conf", name)),
    )
    .context("failed to write dropin to disk")
}

pub fn materialize_boot_info<ENV: Environment + std::fmt::Debug>(
    log: Logger,
    normal_dir: &Path,
    env_dir: &Path,
    network_unit_dir: &Path,
    env: ENV,
    extra_deps: ExtraDependencies,
    mount_unit: MountUnit,
    network_unit_dropin: Option<Dropin<NetworkUnit>>,
) -> Result<()> {
    info!(
        log,
        "Writing environment file {:?} in {:?}:\n{:#?}",
        env_dir,
        Path::new(ENVIRONMENT_FILENAME),
        env
    );
    env.write_systemd_env_file(env_dir, Path::new(ENVIRONMENT_FILENAME))
        .context("failed to write environment file")?;

    for (name, dep) in extra_deps {
        info!(
            log,
            "Writing extra dependency {}: {} requires/after {}", name, dep.source, dep.requires,
        );
        write_extra_dependency(log.clone(), normal_dir, &name, dep)
            .context(format!("failed to write extra dep {}", name))?;
    }

    write_unit_to_disk(
        &Unit {
            unit: Some(mount_unit.unit_section),
            body: Some(UnitBody::Mount(mount_unit.mount_section)),
        },
        log.clone(),
        normal_dir,
        ROOTDISK_MOUNT_SERVICE.as_ref(),
    )
    .context("Failed to write rootdisk mount")?;

    match network_unit_dropin {
        Some(n) => {
            // if the dropin_filename is not provided we we use the target name
            // e.g.:
            //   $something.network translates into $something.network.conf
            //   $something.service translates into $something.service.conf
            let dropin_filename: String = match n.dropin_filename {
                Some(ref filename) => filename.to_string(),
                None => format!("{}.conf", n.target),
            };
            write_dropin_to_disk(
                &n,
                log.clone(),
                network_unit_dir,
                Path::new(&dropin_filename),
            )
            .context("Failed to write network unit drop in file")?;
        }
        None => {}
    }

    Ok(())
}

fn write_unit_to_disk<UNIT: Render>(
    unit: &UNIT,
    log: Logger,
    base_dir: &Path,
    filename: &Path,
) -> Result<()> {
    let unit_file_path = base_dir.join(filename);
    let mut file = std::fs::File::create(&unit_file_path).context(format!(
        "Failed to create unit file {:?} in {:?}",
        filename, base_dir
    ))?;

    let content = unit.render();
    info!(log, "Writing to {:?}:\n{}", unit_file_path, content);
    write!(file, "{}", content).context("Failed to write rendered content to file")?;
    Ok(())
}

pub fn write_dropin_to_disk<UNIT: Render>(
    dropin: &Dropin<UNIT>,
    log: Logger,
    base_dir: &Path,
    filename: &Path,
) -> Result<()> {
    let dropin_path: PathBuf = format!("{}.d", dropin.target).into();
    let unit_dir: PathBuf = base_dir.join(&dropin_path);
    std::fs::create_dir_all(&unit_dir)
        .context(format!("failed to create .d/ for {}", dropin.target))?;

    println!(
        "Writing drop-in {:?} into {:?} for {}",
        filename, unit_dir, dropin.target
    );
    info!(
        log,
        "Writing drop-in {:?} into {:?} for {}", filename, unit_dir, dropin.target
    );

    write_unit_to_disk(&dropin.unit, log, base_dir, &unit_dir.join(filename))
}

#[derive(StructOpt, Debug, Clone)]
pub struct GeneratorArgs {
    pub normal_dir: PathBuf,
    pub early_dir: PathBuf,
    pub late_dir: PathBuf,
}

pub fn setup_generator_test(name: &'static str) -> Result<(PathBuf, GeneratorArgs)> {
    let ts = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .context("Failed to get timestamp")?;
    let tmpdir = std::env::temp_dir().join(format!("test_generator_{}_{:?}", name, ts));

    let normal = tmpdir.join("normal");
    let early = tmpdir.join("early");
    let late = tmpdir.join("late");

    std::fs::create_dir(&tmpdir).context("failed to create tmpdir")?;
    std::fs::create_dir(&normal).context("failed to create normal dir")?;
    std::fs::create_dir(&early).context("failed to create early dir")?;
    std::fs::create_dir(&late).context("failed to create late dir")?;

    symlink("emergency.target", early.join("default.target"))?;

    Ok((
        tmpdir,
        GeneratorArgs {
            normal_dir: normal,
            early_dir: early,
            late_dir: late,
        },
    ))
}

#[cfg(test)]
mod tests {
    use std::time::SystemTime;

    use anyhow::Result;
    use maplit::btreemap;
    use slog::o;
    use systemd::render::NetworkUnit;
    use systemd::render::NetworkUnitMatchSection;

    use super::*;

    #[derive(Clone, Debug, Serialize)]
    struct TestInner {
        #[serde(rename = "REQUIRED_INNER")]
        req_inner: String,
        #[serde(rename = "OPTIONAL_INNER")]
        opt_inner: Option<String>,
    }

    #[derive(Clone, Debug, Serialize)]
    struct TestEnvironment {
        #[serde(rename = "REQUIRED_STRING")]
        req_string: String,

        #[serde(rename = "OPTIONAL_STRING")]
        opt_string: Option<String>,

        #[serde(rename = "REQUIRED_PATH")]
        req_path: PathBuf,

        #[serde(rename = "OPTIONAL_PATH")]
        opt_path: Option<PathBuf>,

        #[serde(flatten)]
        inner: TestInner,
    }
    impl Environment for TestEnvironment {}

    #[test]
    fn test_environment() -> Result<()> {
        let ts = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH)?;
        let tmpdir = std::env::temp_dir().join(format!("test_environment_{:?}", ts));
        std::fs::create_dir(&tmpdir)?;

        let env = TestEnvironment {
            req_string: "s1".to_string(),
            opt_string: Some("s2".to_string()),
            req_path: "s3".into(),
            opt_path: Some("s4".into()),
            inner: TestInner {
                req_inner: "s5".to_string(),
                opt_inner: Some("s6".to_string()),
            },
        };

        assert_eq!(
            env.clone()
                .try_into_map()
                .context("failed to convert env to map")?,
            btreemap! {
                "REQUIRED_STRING".to_string() => "s1".to_string(),
                "OPTIONAL_STRING".to_string() => "s2".to_string(),
                "REQUIRED_PATH".to_string() => "s3".to_string(),
                "OPTIONAL_PATH".to_string() => "s4".to_string(),
                "REQUIRED_INNER".to_string() => "s5".to_string(),
                "OPTIONAL_INNER".to_string() => "s6".to_string(),
            }
        );

        let path = env
            .write_systemd_env_file(&tmpdir, Path::new("test_env_file"))
            .context("failed to write env file")?;

        assert_eq!(path, tmpdir.join(Path::new("test_env_file")));

        let content =
            std::fs::read_to_string(path.clone()).context(format!("Can't read file {:?}", path))?;

        assert_eq!(
            content,
            "\
            OPTIONAL_INNER=s6\n\
            OPTIONAL_PATH=s4\n\
            OPTIONAL_STRING=s2\n\
            REQUIRED_INNER=s5\n\
            REQUIRED_PATH=s3\n\
            REQUIRED_STRING=s1\n\
            "
        );

        let env = TestEnvironment {
            req_string: "s1".to_string(),
            opt_string: None,
            req_path: "s3".into(),
            opt_path: None,
            inner: TestInner {
                req_inner: "s5".to_string(),
                opt_inner: None,
            },
        };

        assert_eq!(
            env.clone()
                .try_into_map()
                .context("failed to convert env to map")?,
            btreemap! {
                "REQUIRED_STRING".to_string() => "s1".to_string(),
                "REQUIRED_PATH".to_string() => "s3".to_string(),
                "REQUIRED_INNER".to_string() => "s5".to_string(),
            }
        );

        let path = env
            .write_systemd_env_file(&tmpdir, Path::new("test_env_file"))
            .context("failed to write env file")?;

        assert_eq!(path, tmpdir.join(Path::new("test_env_file")));

        let content =
            std::fs::read_to_string(path.clone()).context(format!("Can't read file {:?}", path))?;

        assert_eq!(
            content,
            "\
            REQUIRED_INNER=s5\n\
            REQUIRED_PATH=s3\n\
            REQUIRED_STRING=s1\n\
            "
        );

        Ok(())
    }

    #[test]
    fn test_invalid_environment() {
        #[derive(Serialize)]
        struct Inner {
            inside: String,
        }
        #[derive(Serialize)]
        struct Nested {
            inner: Inner,
        }
        impl Environment for Nested {}

        assert!(
            Nested {
                inner: Inner {
                    inside: "s1".to_string(),
                },
            }
            .try_into_map()
            .is_err()
        );

        #[derive(Serialize)]
        struct BadMap {
            map: BTreeMap<String, String>,
        }
        impl Environment for BadMap {}

        assert!(
            BadMap {
                map: BTreeMap::new(),
            }
            .try_into_map()
            .is_err()
        );

        #[derive(Serialize)]
        struct BadNum {
            number: i64,
        }
        impl Environment for BadNum {}

        assert!(BadNum { number: 123 }.try_into_map().is_err());
    }

    #[test]
    fn test_materialize_boot_info() -> Result<()> {
        let log = slog::Logger::root(slog_glog_fmt::default_drain(), o!());
        let ts = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH)?;
        let tmpdir = std::env::temp_dir().join(format!("test_environment_{:?}", ts));
        let env_dir = tmpdir.join("env_dir");
        let deps_dir = tmpdir.join("deps_dir");
        let network_unit_dir = tmpdir.join("network_unit_dir");

        std::fs::create_dir(&tmpdir).context("failed to make tmp dir")?;
        std::fs::create_dir(&env_dir).context("failed to make env_dir")?;
        std::fs::create_dir(&deps_dir).context("failed to make deps")?;
        std::fs::create_dir(&network_unit_dir).context("failed to make network_unit_dir")?;

        let env = TestEnvironment {
            req_string: "s1".to_string(),
            opt_string: Some("s2".to_string()),
            req_path: "s3".into(),
            opt_path: Some("s4".into()),
            inner: TestInner {
                req_inner: "s5".to_string(),
                opt_inner: Some("s6".to_string()),
            },
        };

        materialize_boot_info(
            log,
            &deps_dir,
            &env_dir,
            &network_unit_dir,
            env,
            vec![
                (
                    "extra_1".to_string(),
                    ExtraDependency {
                        source: "source_1.service".into(),
                        requires: "required_1.service".into(),
                    },
                ),
                (
                    "extra_2".to_string(),
                    ExtraDependency {
                        source: "source_2.service".into(),
                        requires: "required_2.service".into(),
                    },
                ),
            ],
            MountUnit {
                unit_section: UnitSection {
                    ..Default::default()
                },
                mount_section: MountSection {
                    what: "/dev/test".into(),
                    where_: "/test_mount".into(),
                    options: None,
                    type_: None,
                },
            },
            Some(Dropin {
                target: "eth.network".into(),
                unit: NetworkUnit {
                    match_section: NetworkUnitMatchSection {
                        name: "*".to_string(),
                        mac_address: "11:22:33:44:55:66".to_string(),
                    },
                },
                dropin_filename: Some("match.conf".to_string()),
            }),
        )
        .context("Failed to materialize_boot_info")?;

        assert_eq!(
            std::fs::read_to_string(env_dir.join(ENVIRONMENT_FILENAME))
                .context("Can't read environment file")?,
            "\
            OPTIONAL_INNER=s6\n\
            OPTIONAL_PATH=s4\n\
            OPTIONAL_STRING=s2\n\
            REQUIRED_INNER=s5\n\
            REQUIRED_PATH=s3\n\
            REQUIRED_STRING=s1\n\
            "
        );

        assert_eq!(
            std::fs::read_to_string(deps_dir.join("source_1.service.d/extra_1.conf"))
                .context("Can't read extra_1.conf file")?,
            "\
            [Unit]\n\
            After=required_1.service\n\
            Requires=required_1.service\n\
            "
        );

        assert_eq!(
            std::fs::read_to_string(deps_dir.join("source_2.service.d/extra_2.conf"))
                .context("Can't read extra_2.conf file")?,
            "\
            [Unit]\n\
            After=required_2.service\n\
            Requires=required_2.service\n\
            "
        );

        assert_eq!(
            std::fs::read_to_string(deps_dir.join("run-fs-control.mount"))
                .context("Can't read run-fs-control.mount file")?,
            "\
            [Unit]\n\
            [Mount]\n\
            What=/dev/test\n\
            Where=/test_mount\n\
            Options=\n\
            "
        );

        assert_eq!(
            std::fs::read_to_string(network_unit_dir.join("eth.network.d/match.conf"))
                .context("Can't read eth.network.d/match.conf file")?,
            "\
            [Match]\n\
            Name=*\n\
            MACAddress=11:22:33:44:55:66\n\
            "
        );

        Ok(())
    }
}
