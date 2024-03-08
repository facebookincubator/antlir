/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use pyo3::create_exception;
use pyo3::prelude::*;

create_exception!(
    signed_source,
    SignedSourceError,
    pyo3::exceptions::PyException
);

#[pymodule]
pub fn signed_source(py: Python<'_>, m: &PyModule) -> PyResult<()> {
    m.add("SignedSourceError", py.get_type::<SignedSourceError>())?;

    /// sign(src)
    /// --
    ///
    /// Signs the input source file. Linters will warn if the signature does not
    /// match the contents.
    /// This is not a security measure, it just discourages people from manually
    /// editing generated files, which is error-prone and usually quickly
    /// overwritten by automation.
    #[pyfn(m)]
    fn sign_source(src: &str) -> PyResult<String> {
        signedsource::sign(src).map_err(|e| SignedSourceError::new_err(e.to_string()))
    }

    #[pyfn(m)]
    fn signed_source_sigil() -> PyResult<&'static str> {
        Ok(signedsource::TOKEN)
    }

    #[pyfn(m)]
    #[pyo3(signature = (src, comment = "#"))]
    fn sign_with_generated_header(src: &str, comment: &str) -> String {
        signedsource::sign_with_generated_header(
            signedsource::Comment::Arbitrary(comment.to_owned()),
            src,
        )
    }

    Ok(())
}
