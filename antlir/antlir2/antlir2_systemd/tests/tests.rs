/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashMap;

use antlir2_systemd::UnitFileState;
use maplit::hashmap;
use pretty_assertions::assert_eq;

#[test]
fn list_unit_files() {
    let units =
        antlir2_systemd::list_unit_files("/layer").expect("failed to list unit files in /layer");
    let names_to_states = units
        .iter()
        .map(|unit| (unit.name(), unit.state()))
        .collect::<HashMap<_, _>>();
    assert_eq!(
        names_to_states,
        hashmap! {
            "foo.service" => UnitFileState::Static,
            "bar@.service" => UnitFileState::Indirect,
            "bar@baz.service" => UnitFileState::Enabled,
            "bar@qux.service" => UnitFileState::Enabled,
            "x.socket" => UnitFileState::Static,
            "y.socket" => UnitFileState::Enabled,
        }
    );
}
