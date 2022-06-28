/*
 * Copyright (c) Meta Platforms, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use slog::{debug, o, Logger};
use tss_esapi::{
    interface_types::algorithm::HashingAlgorithm,
    structures::{PcrSelectionListBuilder, PcrSlot},
    Context as TssContext, Tcti,
};
use uefi_eventlog::Parser as EventlogParser;

mod eventlog {
    use serde::Deserialize;
    use std::collections::BTreeMap;
    use uefi_eventlog::Event;

    pub const UEFI_EVENTLOG_PATH: &str = "/sys/kernel/security/tpm0/binary_bios_measurements";

    #[derive(PartialEq, Eq, PartialOrd, Ord, Copy, Clone, Deserialize, Debug)]
    #[serde(rename_all = "lowercase")]
    pub enum DigestAlgorithm {
        Sha1,
        Sha256,
        Sha384,
        Sha512,
    }

    fn deserialize_as_base64<'d, D>(deserialize: D) -> Result<Vec<u8>, D::Error>
    where
        D: serde::Deserializer<'d>,
    {
        let hexstr = String::deserialize(deserialize)?;
        base64::decode(&hexstr).map_err(serde::de::Error::custom)
    }

    #[derive(Clone, Deserialize, Debug)]
    pub struct Digest {
        method: DigestAlgorithm,

        #[allow(dead_code)]
        #[serde(deserialize_with = "deserialize_as_base64")]
        digest: Vec<u8>,
    }

    #[derive(derive_more::Index, Debug)]
    pub struct DigestMap(BTreeMap<DigestAlgorithm, Digest>);

    impl TryFrom<Event> for DigestMap {
        type Error = serde_json::Error;

        fn try_from(event: Event) -> Result<Self, Self::Error> {
            // amazingly the uefi_eventlog::Event::digests does not present any interface
            // to extract data and all fields are private; the only thing it derives is
            // serde serialization; so use that
            let json = serde_json::to_string(&event.digests)?;
            let digests: Vec<Digest> = serde_json::from_str(&json)?;

            let mut map = BTreeMap::new();
            for dig in digests {
                map.insert(dig.method, dig);
            }
            Ok(DigestMap(map))
        }
    }
}

#[test]
fn can_use_tpm_device() {
    let log = Logger::root(slog_glog_fmt::default_drain(), o!());

    let mut context =
        TssContext::new(Tcti::from_environment_variable().expect("Failed to get TCTI"))
            .expect("Failed to create Context");

    let pcr_selection = PcrSelectionListBuilder::new()
        .with_selection(HashingAlgorithm::Sha256, &[PcrSlot::Slot4])
        .build()
        .expect("Failed to build PcrSelectionList");

    let (_counter, _read_pcr_list, digests) = context
        .pcr_read(pcr_selection)
        .expect("Call to pcr_read failed");

    debug!(log, "sha256:4 = {:?}", digests.value()[0]);
}

#[test]
fn can_read_eventlog() {
    use fallible_iterator::FallibleIterator;
    let log = Logger::root(slog_glog_fmt::default_drain(), o!());

    let evlog = std::fs::File::open(eventlog::UEFI_EVENTLOG_PATH)
        .expect("failed to open UEFI event log file");
    let mut parser = EventlogParser::new(evlog);

    while let Some(event) = parser.next().expect("failed to get events") {
        if event.pcr_index == 0 {
            let evtype = event.event.clone();
            let digests: eventlog::DigestMap =
                event.try_into().expect("failed to convert event digest");

            debug!(
                log,
                "event <{:?}> sha1:0 = {:?}",
                evtype,
                digests[&eventlog::DigestAlgorithm::Sha1]
            );
        }
    }
}
