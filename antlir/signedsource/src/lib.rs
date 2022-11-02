/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use md5::Digest;
use md5::Md5;
use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum Error {
    #[error("missing SignedSource token")]
    MissingToken,
}

pub type Result<R> = std::result::Result<R, Error>;

pub static TOKEN: &str = "<<SignedSource::*O*zOeWoEQle#+L!plEphiEmie@IsG>>";

pub fn sign(src: &str) -> Result<String> {
    if !src.contains(TOKEN) {
        return Err(Error::MissingToken);
    }
    let md5hex = format!("SignedSource<<{:x}>>", Md5::digest(src));
    Ok(src.replace(TOKEN, &md5hex))
}

#[derive(Debug, Copy, Clone)]
pub enum Comment {
    Hash,
    Python,
    Rust,
}

impl std::fmt::Display for Comment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Self::Hash | Self::Python => "#",
                Self::Rust => "//",
            }
        )
    }
}

pub fn sign_with_generated_header(comment: Comment, src: &str) -> String {
    let mut s = format!("{} @{} {}\n", comment, "generated", TOKEN);
    s.push_str(src);
    sign(&s).expect("token is definitely there")
}

#[cfg(test)]
mod tests {
    use super::TOKEN;
    use super::*;

    #[test]
    fn simple() {
        assert_eq!(
            "hello SignedSource<<9f94b0b1eddcee39813128cd51ef0e47>> world!",
            sign(&format!("hello {TOKEN} world!")).expect("has token"),
        );
    }

    #[test]
    fn missing_token() {
        assert_eq!(
            Error::MissingToken,
            sign("hello world!").expect_err("missing token"),
        );
    }
}
