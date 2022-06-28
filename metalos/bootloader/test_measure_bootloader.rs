// Copyright (c) Meta Platforms, Inc. and its affiliates.
// This source code is licensed under the MIT license found in the
// LICENSE file in the root directory of this source tree.

use slog::{debug, o, Logger};
use tss_esapi::{interface_types::algorithm::HashingAlgorithm, structures::PcrSlot};

mod tpm {
    use anyhow::{Context, Result};
    use tss_esapi::{
        interface_types::algorithm::HashingAlgorithm,
        structures::{digest_list::DigestList, PcrSelectionListBuilder, PcrSlot},
        Context as TssContext, Tcti,
    };

    /// Read a PCR value from the TPM for the given hash algo
    pub fn get_pcr(pcr_slot: PcrSlot, algo: HashingAlgorithm) -> Result<DigestList> {
        let mut context =
            TssContext::new(Tcti::from_environment_variable().context("failed to get TCTI")?)
                .context("failed to create Context")?;

        let pcr_selection = PcrSelectionListBuilder::new()
            .with_selection(algo, &[pcr_slot])
            .build()
            .context("failed to build PcrSelectionList")?;

        let (_counter, _read_pcr_list, digests) = context
            .pcr_read(pcr_selection)
            .context("call to pcr_read failed")?;

        Ok(digests)
    }
}

mod eventlog {
    use anyhow::{Context as ErrContext, Result};
    use ring::{digest, digest::Context};
    use serde::Deserialize;
    use std::collections::BTreeMap;
    use uefi_eventlog::{Event, Parser};

    pub const UEFI_EVENTLOG_PATH: &str = "/sys/kernel/security/tpm0/binary_bios_measurements";

    #[derive(PartialEq, Eq, PartialOrd, Ord, Copy, Clone, Deserialize, Debug)]
    #[serde(rename_all = "lowercase")]
    pub enum DigestAlgorithm {
        Sha1,
        Sha256,
        Sha384,
        Sha512,
    }

    impl From<DigestAlgorithm> for &'static digest::Algorithm {
        fn from(algo: DigestAlgorithm) -> Self {
            match algo {
                DigestAlgorithm::Sha1 => &digest::SHA1_FOR_LEGACY_USE_ONLY,
                DigestAlgorithm::Sha256 => &digest::SHA256,
                DigestAlgorithm::Sha384 => &digest::SHA384,
                DigestAlgorithm::Sha512 => &digest::SHA512,
            }
        }
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

        #[serde(deserialize_with = "deserialize_as_base64")]
        pub digest: Vec<u8>,
    }

    #[derive(derive_more::Index, Debug)]
    pub struct DigestMap(BTreeMap<DigestAlgorithm, Digest>);

    impl TryFrom<&Event> for DigestMap {
        type Error = serde_json::Error;

        fn try_from(event: &Event) -> Result<Self, Self::Error> {
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

    /// Returns the full list of UEFI eventlog items
    pub fn get_events() -> Result<Vec<Event>> {
        use fallible_iterator::FallibleIterator;

        let evlog = std::fs::File::open(UEFI_EVENTLOG_PATH)
            .context("failed to open UEFI event log file")?;

        let mut events = vec![];
        let mut parser = Parser::new(evlog);

        while let Some(event) = parser.next().context("failed to parse events")? {
            events.push(event);
        }
        Ok(events)
    }

    /// Takes in the whole UEFI event log and reproduces a specified PCR value
    /// based on the given hashing algorithm.
    pub fn reproduce_pcr(
        pcr_index: u32,
        algo: DigestAlgorithm,
        events: &[Event],
    ) -> Result<Vec<u8>> {
        let context_algo: &digest::Algorithm = algo.into();
        let mut pcr = vec![0u8; context_algo.output_len];

        for event in events {
            if event.pcr_index != pcr_index {
                continue;
            }

            let digests: DigestMap = event
                .try_into()
                .context("failed to to convert event digest")?;

            let mut context = Context::new(context_algo);
            context.update(pcr.as_ref());
            context.update(digests[&algo].digest.as_ref());
            pcr = context.finish().as_ref().to_vec();
        }

        Ok(pcr)
    }
}

#[test]
fn can_use_tpm_device() {
    let log = Logger::root(slog_glog_fmt::default_drain(), o!());

    let digests = tpm::get_pcr(PcrSlot::Slot4, HashingAlgorithm::Sha256).unwrap();
    debug!(log, "sha256:4 = {:?}", digests.value()[0]);
}

#[test]
fn can_read_eventlog() {
    let log = Logger::root(slog_glog_fmt::default_drain(), o!());

    for event in &eventlog::get_events().unwrap() {
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

#[test]
fn validate_pcr4() {
    use hex;
    use uefi_eventlog::EventType;

    let log = Logger::root(slog_glog_fmt::default_drain(), o!());

    let events = eventlog::get_events().expect("failed to get logged events");
    let eventlog_pcr4 = eventlog::reproduce_pcr(4, eventlog::DigestAlgorithm::Sha1, &events)
        .expect("failed to reproduce pcr4 from events");

    let digests =
        tpm::get_pcr(PcrSlot::Slot4, HashingAlgorithm::Sha1).expect("failed to get pcr4 from tpm");
    let tpm_pcr4 = digests.value()[0].value();

    assert_eq!(
        tpm_pcr4, eventlog_pcr4,
        "cannot validate eventlog thru TPM pcr4"
    );

    let bootloader_hash = std::fs::read_to_string(
        std::env::var("BOOTLOADER_HASH_FILE").expect("failed to get env BOOTLOADERRR_HASH_FILE"),
    )
    .expect("failed to read bootloader hash file")
    .trim_end()
    .to_owned();

    // now look for the hash in the eventlog and compare
    debug!(log, "expected bootloader hash: {}", &bootloader_hash);
    let expected = hex::decode(&bootloader_hash).unwrap();

    assert!(
        events.iter().any(|e| {
            if e.pcr_index != 4 || e.event != EventType::EFIBootServicesApplication {
                return false;
            }

            let digests: eventlog::DigestMap =
                e.try_into().expect("failed to to convert event digest");
            let event_hash = digests[&eventlog::DigestAlgorithm::Sha1].digest.clone();

            debug!(
                log,
                "found eventlog hash for pcr4: {:?} -> {}",
                e.event,
                hex::encode(&event_hash)
            );
            event_hash == expected
        }),
        "could not find the bootloader hash in eventlog, expected {}",
        &bootloader_hash
    );
}
