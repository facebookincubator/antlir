/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Rust side of Buck2's action error handlers.
//!
//! Provides a standard way to report structured errors from Rust and keep that
//! structure in Buck2.
//!
//! Today, these error details only appear in Buck2 datasets and build reports,
//! but should appear more prominently in the buck2 cli/ui in the future.
//!
//! https://www.internalfb.com/intern/staticdocs/buck2/docs/rule_authors/action_error_handler/

use std::fmt::Display;

use serde::Serialize;
use typed_builder::TypedBuilder;

#[derive(TypedBuilder, Debug, Clone, Serialize)]
pub struct SubError {
    #[builder(setter(transform=|s: impl Display| s.to_string()))]
    category: String,
    #[builder(default, setter(transform=|s: impl Display| Some(s.to_string())))]
    message: Option<String>,
    #[builder(default)]
    locations: Vec<Location>,
}

#[derive(TypedBuilder, Debug, Clone, Serialize)]
pub struct Location {
    file: String,
    #[builder(default, setter(strip_option))]
    line: Option<u32>,
}

impl SubError {
    pub fn from_err(category: impl Display, err: impl std::error::Error) -> Self {
        Self {
            category: category.to_string(),
            message: Some(err.to_string()),
            locations: Default::default(),
        }
    }

    pub fn log(&self) {
        // Dump this to stderr. Unfortunately the action error handler only gets
        // access to the stdout/err text and not any other artifacts, so we have
        // to deal with it being mixed in with other output, hence this prefix.
        eprintln!(
            "antlir2_error_handler: {}",
            serde_json::to_string(self).expect("SubError is always json-serializable")
        );
    }
}
