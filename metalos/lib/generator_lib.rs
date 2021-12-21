use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use serde::Serialize;
use slog::{info, Logger};
use systemd::render::{MountSection, Render, Unit, UnitBody, UnitSection};
use systemd::UnitName;

pub const ENVIRONMENT_FILENAME: &str = "metalos_environment";

pub const ROOTDISK_MOUNT_SERVICE: &str = "rootdisk.mount";

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
/// it says that `target` requires `requires` and that `target` is after `requires`
#[derive(Debug, PartialEq)]
pub struct ExtraDependency {
    pub source: UnitName,
    pub requires: UnitName,
}

/// A mapping of dependency name (must be valid as a filename) and a extra dependency
pub type ExtraDependencies = BTreeMap<String, ExtraDependency>;

#[derive(Debug, PartialEq, PartialOrd)]
pub struct MountUnit {
    pub unit_section: UnitSection,
    pub mount_section: MountSection,
}

/// This represents a dropin unit that the generator can make. The unit inside can be
/// written to disk with the `systemd::render::Render` trait and we also hold a name
/// of the unit that we are targeting
struct Dropin {
    target: UnitName,
    unit: Unit,
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
    env: ENV,
    extra_deps: ExtraDependencies,
    mount_unit: MountUnit,
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

    Ok(())
}

fn write_unit_to_disk(unit: &Unit, log: Logger, base_dir: &Path, filename: &Path) -> Result<()> {
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

fn write_dropin_to_disk(
    dropin: &Dropin,
    log: Logger,
    base_dir: &Path,
    filename: &Path,
) -> Result<()> {
    let dropin_path: PathBuf = format!("{}.d", dropin.target).into();
    let service_dir: PathBuf = base_dir.join(&dropin_path);
    std::fs::create_dir_all(&service_dir)
        .context(format!("failed to create .d/ for {}", dropin.target))?;

    println!(
        "Writing drop-in {:?} into {:?} for {}",
        filename, service_dir, dropin.target
    );
    info!(
        log,
        "Writing drop-in {:?} into {:?} for {}", filename, service_dir, dropin.target
    );

    write_unit_to_disk(&dropin.unit, log, base_dir, &service_dir.join(filename))
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use maplit::btreemap;
    use slog::o;
    use std::time::SystemTime;

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

        std::fs::create_dir(&tmpdir).context("failed to make tmp dir")?;
        std::fs::create_dir(&env_dir).context("failed to make env_dir")?;
        std::fs::create_dir(&deps_dir).context("failed to make deps")?;

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
            env,
            btreemap! {
                "extra_1".to_string() => ExtraDependency {
                    source: "source_1.service".into(),
                    requires: "required_1.service".into(),
                },
                "extra_2".to_string() => ExtraDependency {
                    source: "source_2.service".into(),
                    requires: "required_2.service".into(),
                },
            },
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
            std::fs::read_to_string(deps_dir.join("rootdisk.mount"))
                .context("Can't read rootdisk.mount file")?,
            "\
            [Unit]\n\
            [Mount]\n\
            What=/dev/test\n\
            Where=/test_mount\n\
            Options=\n\
            "
        );

        Ok(())
    }
}
