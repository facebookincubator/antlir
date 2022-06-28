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
