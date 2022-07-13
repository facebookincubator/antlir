use itertools::Itertools;
use serde::ser::SerializeSeq;
use serde::ser::Serializer;
use serde::Serialize;
use service_shape::binary_t;
use service_shape::cmd_t;
use service_shape::dependency_mode_t;
use service_shape::exec_t;
use service_shape::resource_limits_t;
use service_shape::restart_mode_t;
use service_shape::service_t;
use service_shape::service_type_t;
use std::collections::BTreeMap;
use systemd::UnitName;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("'{setting}' has an invalid value '{value}': {message}")]
    InvalidSetting {
        setting: &'static str,
        value: String,
        message: String,
    },
}

type Result<R> = std::result::Result<R, Error>;

#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct UnitFile {
    unit: UnitSection,
    service: ServiceSection,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
struct UnitSection {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    after: Vec<UnitName>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    requires: Vec<UnitName>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct ServiceSection {
    #[serde(skip)]
    pub(crate) environment: Environment,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) exec_start: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) exec_start_pre: Vec<String>,
    pub(crate) group: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) restart: Option<restart_mode_t>,
    #[serde(rename = "Type")]
    pub(crate) service_type: service_type_t,
    pub(crate) user: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) bind_read_only_paths: Vec<String>,
    // resource_limits_t is flattened to these fields
    #[serde(rename = "LimitNOFILE", skip_serializing_if = "Option::is_none")]
    pub(crate) open_fds: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) memory_max: Option<usize>,
}

#[derive(Debug, Default)]
struct ResourceLimits {
    open_fds: Option<usize>,
    memory_max: Option<usize>,
}

impl TryFrom<resource_limits_t> for ResourceLimits {
    type Error = Error;

    #[deny(unused_variables)]
    fn try_from(x: resource_limits_t) -> Result<Self> {
        let resource_limits_t {
            open_fds,
            memory_max_bytes,
        } = x;
        Ok(Self {
            open_fds: open_fds.map(|x| x.try_into()).transpose().map_err(|_| {
                Error::InvalidSetting {
                    setting: "open_fds",
                    value: open_fds.expect("this is definitely Some").to_string(),
                    message: "open_fds must be positive".to_string(),
                }
            })?,
            memory_max: memory_max_bytes
                .map(|x| x.try_into())
                .transpose()
                .map_err(|_| Error::InvalidSetting {
                    setting: "memory_max_bytes",
                    value: memory_max_bytes
                        .expect("this is definitely Some")
                        .to_string(),
                    message: "memory_max_bytes must be positive".to_string(),
                })?,
        })
    }
}

fn cmd_to_setting(cmd: cmd_t) -> String {
    let binary = match cmd.binary {
        binary_t::target_t(target) => {
            format!("/metalos/bin/{}", target.name.replace('/', "."))
        }
        binary_t::String(s) => s,
    };
    let mut v = vec![binary];
    v.extend(cmd.args);
    v.into_iter()
        .map(|s| shell_words::quote(&s).to_string())
        .join(" ")
}

impl TryFrom<exec_t> for ServiceSection {
    type Error = Error;

    #[deny(unused_variables)]
    fn try_from(x: exec_t) -> Result<Self> {
        let exec_t {
            environment,
            pre,
            resource_limits,
            restart,
            run,
            runas,
            service_type,
        } = x;
        let resource_limits = resource_limits
            .map(ResourceLimits::try_from)
            .transpose()?
            .unwrap_or_default();
        Ok(Self {
            environment: Environment(environment),
            exec_start: run.into_iter().map(cmd_to_setting).collect(),
            exec_start_pre: pre.into_iter().map(cmd_to_setting).collect(),
            group: runas.group,
            restart,
            service_type,
            user: runas.user,
            bind_read_only_paths: vec![],
            open_fds: resource_limits.open_fds,
            memory_max: resource_limits.memory_max,
        })
    }
}

impl TryFrom<service_t> for UnitFile {
    type Error = Error;

    #[deny(unused_variables)]
    fn try_from(x: service_t) -> Result<Self> {
        let service_t {
            dependencies,
            exec_info,
            name,
            config_generator: _,
            certificates,
        } = x;
        let mut after = Vec::new();
        let mut requires = Vec::new();
        for dep in dependencies {
            match dep.mode {
                dependency_mode_t::AFTER_ONLY => {
                    after.push(dep.unit.into());
                }
                dependency_mode_t::REQUIRES_ONLY => {
                    requires.push(dep.unit.into());
                }
                dependency_mode_t::REQUIRES_AND_AFTER => {
                    requires.push(dep.unit.clone().into());
                    after.push(dep.unit.into());
                }
            }
        }
        let mut service: ServiceSection = exec_info.try_into()?;
        #[cfg(facebook)]
        crate::facebook::service_certs::add_cert_settings(&mut service, certificates, name);
        Ok(Self {
            unit: UnitSection { after, requires },
            service,
        })
    }
}

// At some point it would be nice to bootcamp moving this into `serde_systemd`
// or a companion crate, but I(vmagro) want to wait a little bit on that until I
// can collect a more useful set of primitives and specialized types (based
// primarily on usage that will spring up in this crate)
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Environment(pub(crate) BTreeMap<String, String>);

impl Serialize for Environment {
    fn serialize<S>(&self, ser: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut seq = ser.serialize_seq(Some(self.0.len()))?;
        for (k, v) in &self.0 {
            seq.serialize_element(&format!("{}={}", k, v))?;
        }
        seq.end()
    }
}
