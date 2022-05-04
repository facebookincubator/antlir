use send_events::Event;

pub struct RamdiskReady {}

impl TryInto<Event> for RamdiskReady {
    type Error = anyhow::Error;

    fn try_into(self) -> Result<Event, Self::Error> {
        Ok(Event {
            name: "RAMDISK_IMAGE.READY".to_string(),
            payload: None,
        })
    }
}
