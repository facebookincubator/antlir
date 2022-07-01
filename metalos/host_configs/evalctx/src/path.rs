/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::Path;
use std::path::PathBuf;

pub trait PathExt {
    /// Joining absolute paths is annoying, so add an extension trait for
    /// `force_join` which makes it easy.
    fn force_join<P: AsRef<Path>>(&self, path: P) -> PathBuf;
}

impl<T> PathExt for T
where
    T: AsRef<Path>,
{
    fn force_join<P: AsRef<Path>>(&self, path: P) -> PathBuf {
        match path.as_ref().is_absolute() {
            false => self.as_ref().join(path),
            true => self.as_ref().join(
                path.as_ref()
                    .strip_prefix("/")
                    .expect("absolute paths will always start with /"),
            ),
        }
    }
}
