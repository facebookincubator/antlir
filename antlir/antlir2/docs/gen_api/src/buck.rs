/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::process::Command;

use anyhow::ensure;
use anyhow::Context;
use anyhow::Result;
use buck_label::Label;
use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(from = "Vec<Element>")]
pub(crate) struct Doc {
    pub(crate) elements: Vec<Element>,
}

impl From<Vec<Element>> for Doc {
    fn from(elements: Vec<Element>) -> Self {
        Self { elements }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct Element {
    pub(crate) id: Id,
    pub(crate) item: Item,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) struct Id {
    pub(crate) name: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum Item {
    Function(Function),
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct Function {
    pub(crate) docs: Option<ItemDocs>,
    pub(crate) params: Vec<Param>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct ItemDocs {
    pub(crate) summary: Option<String>,
    pub(crate) details: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum Param {
    NoArgs,
    OnlyNamedAfter,
    Arg(Arg),
    Kwargs,
    Args,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) struct Arg {
    pub(crate) name: String,
    #[serde(rename(deserialize = "type"))]
    pub(crate) ty: String,
    pub(crate) docs: Option<ItemDocs>,
    pub(crate) default_value: Option<String>,
}

pub(crate) fn starlark_doc(file: Label) -> Result<Doc> {
    let out = Command::new("buck2")
        .arg("docs")
        .arg("starlark")
        .arg("--format=json")
        .arg(&file)
        .output()
        .context("while running buck2 docs")?;
    ensure!(out.status.success(), "buck2 docs failed");
    serde_json::from_slice(&out.stdout).context("while deserializing")
}
