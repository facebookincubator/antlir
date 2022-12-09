/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#[macro_export]
macro_rules! wrap_err {
    ($wrapper:ident, $error:ty) => {
        wrap_err_for_py!($wrapper, $error, ::pyo3::exceptions::PyException);
    };
    ($wrapper:ident, $error:ty, $pyexc:ty) => {
        #[derive(Debug)]
        pub struct $wrapper($error);

        impl From<$error> for $wrapper {
            fn from(e: $error) -> $wrapper {
                $wrapper(e)
            }
        }

        impl From<$wrapper> for ::pyo3::PyErr {
            fn from(error: $wrapper) -> Self {
                <$pyexc>::new_err(format!("{:?}", error.0))
            }
        }
    };
}
