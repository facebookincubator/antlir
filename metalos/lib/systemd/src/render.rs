use std::path::PathBuf;

use crate::UnitName;

pub trait Render {
    fn render(&self) -> String;

    fn add_header(target: &mut String, name: &str) {
        target.push_str(&format!("[{}]\n", name));
    }

    fn add_kv<T: std::fmt::Display>(target: &mut String, key: &str, value: T) {
        target.push_str(&format!("{}={}\n", key, value));
    }

    fn add_optional_kv<T: std::fmt::Display>(target: &mut String, key: &str, value: Option<T>) {
        if let Some(value) = value {
            target.push_str(&format!("{}={}\n", key, value));
        }
    }

    fn add_optional_renderable<T: Render>(target: &mut String, thing: Option<&T>) {
        if let Some(thing) = thing {
            target.push_str(&thing.render());
        }
    }

    fn add_renderable<T: Render>(target: &mut String, thing: &T) {
        target.push_str(&thing.render());
    }
}

#[derive(Debug, Default, PartialEq, PartialOrd)]
pub struct UnitSection {
    pub after: Option<UnitName>,
    pub requires: Option<UnitName>,
}

impl Render for UnitSection {
    fn render(&self) -> String {
        let mut out = String::new();
        Self::add_header(&mut out, "Unit");
        Self::add_optional_kv(&mut out, "After", self.after.as_ref());
        Self::add_optional_kv(&mut out, "Requires", self.requires.as_ref());
        out
    }
}

#[derive(Debug, PartialEq, PartialOrd)]
pub struct MountSection {
    pub what: PathBuf,
    pub where_: PathBuf,
    pub options: Option<String>,
    pub type_: Option<String>,
}

impl Render for MountSection {
    fn render(&self) -> String {
        let mut out = String::new();
        Self::add_header(&mut out, "Mount");
        Self::add_kv(&mut out, "What", &self.what.to_string_lossy());
        Self::add_kv(&mut out, "Where", &self.where_.to_string_lossy());
        Self::add_kv(
            &mut out,
            "Options",
            match &self.options {
                Some(opts) => opts,
                None => "",
            },
        );
        Self::add_optional_kv(&mut out, "Type", self.type_.as_ref());
        out
    }
}

pub enum UnitBody {
    Mount(MountSection),
}

impl Render for UnitBody {
    fn render(&self) -> String {
        match self {
            Self::Mount(s) => s.render(),
        }
    }
}

pub struct Unit {
    pub unit: Option<UnitSection>,
    pub body: Option<UnitBody>,
}

impl Render for Unit {
    fn render(&self) -> String {
        let mut out = String::new();
        Self::add_optional_renderable(&mut out, self.unit.as_ref());
        Self::add_optional_renderable(&mut out, self.body.as_ref());
        out
    }
}

/// Represents systemd-networkd's configuration file as per
/// https://man7.org/linux/man-pages/man8/systemd-networkd.8.html
#[derive(Debug, Default, PartialEq, PartialOrd)]
pub struct NetworkUnit {
    pub match_section: NetworkUnitMatchSection,
}

impl Render for NetworkUnit {
    fn render(&self) -> String {
        let mut out = String::new();
        Self::add_renderable(&mut out, &self.match_section);
        out
    }
}

#[derive(Debug, Default, PartialEq, PartialOrd)]
pub struct NetworkUnitMatchSection {
    pub name: String,
    pub mac_address: String,
}

/// Renders NetworkUnit as per
/// https://man7.org/linux/man-pages/man5/systemd.network.5.html
impl Render for NetworkUnitMatchSection {
    fn render(&self) -> String {
        let mut out = String::new();
        Self::add_header(&mut out, "Match");
        Self::add_kv(&mut out, "Name", self.name.clone());
        Self::add_kv(&mut out, "MACAddress", self.mac_address.clone());
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_unit_section() {
        assert_eq!(
            UnitSection {
                after: Some("after_value".into()),
                requires: Some("requires_value".into()),
            }
            .render(),
            "\
            [Unit]\n\
            After=after_value\n\
            Requires=requires_value\n\
            "
        );
        assert_eq!(
            UnitSection {
                after: None,
                requires: Some("requires_value".into()),
            }
            .render(),
            "\
            [Unit]\n\
            Requires=requires_value\n\
            "
        );
        assert_eq!(
            UnitSection {
                after: Some("after_value".into()),
                requires: None,
            }
            .render(),
            "\
            [Unit]\n\
            After=after_value\n\
            "
        );
        assert_eq!(
            UnitSection {
                after: None,
                requires: None,
            }
            .render(),
            "\
            [Unit]\n\
            "
        );
    }

    #[test]
    fn test_render_mount_section() {
        assert_eq!(
            MountSection {
                what: "/dev/test".into(),
                where_: "/test/mountpoint".into(),
                options: Some("ro,something".to_string()),
                type_: Some("btrfs".to_string()),
            }
            .render(),
            "\
            [Mount]\n\
            What=/dev/test\n\
            Where=/test/mountpoint\n\
            Options=ro,something\n\
            Type=btrfs\n\
            "
        );
        assert_eq!(
            MountSection {
                what: "/dev/test".into(),
                where_: "/test/mountpoint".into(),
                options: None,
                type_: Some("btrfs".to_string()),
            }
            .render(),
            "\
            [Mount]\n\
            What=/dev/test\n\
            Where=/test/mountpoint\n\
            Options=\n\
            Type=btrfs\n\
            "
        );
        assert_eq!(
            MountSection {
                what: "/dev/test".into(),
                where_: "/test/mountpoint".into(),
                options: None,
                type_: None
            }
            .render(),
            "\
            [Mount]\n\
            What=/dev/test\n\
            Where=/test/mountpoint\n\
            Options=\n\
            "
        );
    }

    #[test]
    fn test_render_unit() {
        // test with every field populated
        let test = Unit {
            unit: Some(UnitSection {
                after: Some("after_value".into()),
                requires: Some("requires_value".into()),
            }),
            body: Some(UnitBody::Mount(MountSection {
                what: "/dev/test".into(),
                where_: "/test/mountpoint".into(),
                options: Some("ro,something".to_string()),
                type_: Some("btrfs".to_string()),
            })),
        };

        assert_eq!(
            test.render(),
            "\
            [Unit]\n\
            After=after_value\n\
            Requires=requires_value\n\
            [Mount]\n\
            What=/dev/test\n\
            Where=/test/mountpoint\n\
            Options=ro,something\n\
            Type=btrfs\n\
            "
        );
    }

    #[test]
    fn test_render_network_file() {
        let test = NetworkUnit {
            match_section: NetworkUnitMatchSection {
                name: "eth*".to_string(),
                mac_address: "11:22:33:44:55:66".to_string(),
            },
        };
        assert_eq!(
            test.render(),
            "\
            [Match]\n\
            Name=eth*\n\
            MACAddress=11:22:33:44:55:66\n\
            "
        )
    }
}
