/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeSet;
use std::path::PathBuf;

use serde::Deserialize;
use serde::Serialize;

use crate::Result;

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Plan {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) dnf_transaction: Option<DnfTransaction>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) fbpkg_transaction: Option<FbpkgTransaction>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) other: Vec<serde_json::Value>,
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
                Item::FbpkgTransaction(tx) => {
                    if plan.fbpkg_transaction.is_some() {
                        return Err(anyhow::anyhow!(
                            "impossibly ended up with more than one FbpkgTransaction"
                        )
                        .into());
                    }
                    plan.fbpkg_transaction = Some(tx);
                }
                Item::Other(o) => {
                    plan.other.push(o);
                }
            }
        }
        Ok(plan)
    }

    pub fn dnf_transaction(&self) -> Option<&DnfTransaction> {
        self.dnf_transaction.as_ref()
    }

    pub fn fbpkg_transaction(&self) -> Option<&FbpkgTransaction> {
        self.fbpkg_transaction.as_ref()
    }

    pub fn others(&self) -> &[serde_json::Value] {
        &self.other
    }
}

pub type Nevra = String;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub struct InstallPackage {
    pub nevra: Nevra,
    pub repo: Option<String>,
    pub reason: RpmReason,
}

#[derive(
    Debug,
    Copy,
    Clone,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Deserialize,
    Serialize
)]
#[serde(rename_all = "kebab-case")]
pub enum RpmReason {
    Clean,
    Dependency,
    Group,
    Unknown,
    User,
    WeakDependency,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DnfTransaction {
    pub install: BTreeSet<InstallPackage>,
    pub remove: BTreeSet<Nevra>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct InstallFbpkg {
    pub name: String,
    pub tag: String,
    pub dst: Option<PathBuf>,
    pub organize: bool,
    pub organize_alias: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FbpkgTransaction {
    pub install: Vec<InstallFbpkg>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum Item {
    DnfTransaction(DnfTransaction),
    FbpkgTransaction(FbpkgTransaction),
    Other(serde_json::Value),
}
