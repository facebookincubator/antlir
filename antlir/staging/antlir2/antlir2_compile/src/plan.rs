/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeSet;

use serde::Deserialize;
use serde::Serialize;

use crate::Result;

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Plan {
    #[serde(skip_serializing_if = "Option::is_none")]
    dnf_transaction: Option<DnfTransaction>,
}

impl Plan {
    pub fn from_items<I>(items: I) -> Result<Self>
    where
        I: IntoIterator<Item = Item>,
    {
        let mut plan = Plan::default();
        for item in items {
            match item {
                Item::DnfTransaction(tx) => {
                    if plan.dnf_transaction.is_some() {
                        return Err(anyhow::anyhow!(
                            "impossibly ended up with more than one DnfTransaction"
                        )
                        .into());
                    }
                    plan.dnf_transaction = Some(tx);
                }
                Item::None => {}
            }
        }
        Ok(plan)
    }
}

pub type Nevra = String;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub struct InstallPackage {
    pub(crate) nevra: Nevra,
    pub(crate) repo: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DnfTransaction {
    pub(crate) install: BTreeSet<InstallPackage>,
    pub(crate) remove: BTreeSet<Nevra>,
}

#[derive(Debug, Clone)]
pub enum Item {
    None,
    DnfTransaction(DnfTransaction),
}
