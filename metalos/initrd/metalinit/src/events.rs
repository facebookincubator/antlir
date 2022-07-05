use send_events::Event;
use serde_json::json;
use std::path::Path;

pub struct RamdiskReady {}

impl From<RamdiskReady> for Event {
    fn from(_ev: RamdiskReady) -> Self {
        Self {
            name: "METALOS_INITRD.READY".to_string(),
            payload: None,
        }
    }
}

pub struct StagedConfigs {}

impl From<StagedConfigs> for Event {
    fn from(_ev: StagedConfigs) -> Self {
        Self {
            name: "METALOS_INITRD.STAGED_CONFIGS".to_string(),
            payload: None,
        }
    }
}

pub struct StartingSwitchroot<'a> {
    pub path: &'a Path,
}

impl<'a> From<StartingSwitchroot<'a>> for Event {
    fn from(ev: StartingSwitchroot<'a>) -> Self {
        Self {
            name: "METALOS_INITRD.STARTING_SWITCHROOT".to_string(),
            payload: Some(json!({"path": ev.path})),
        }
    }
}

pub struct Failure<'a> {
    pub error: &'a anyhow::Error,
}

impl<'a> From<Failure<'a>> for Event {
    fn from(ev: Failure<'a>) -> Self {
        Self {
            name: "METALOS_INITRD.FAILURE".to_string(),
            payload: Some(json!({
                "message": format!("{}", ev.error),
                "full": format!("{:?}", ev.error),
            })),
        }
    }
}
