/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fs::File;

use crate::Checksum;
use crate::Result;

pub trait Object: Sized {
    fn from_file(file: File) -> Result<Self>;

    fn to_file(&self, file: &mut File) -> Result<()>;

    fn checksum(&self) -> Result<Checksum<Self>>;
}

#[macro_export]
macro_rules! json_object {
    ($t:ty) => {
        impl $crate::Object for $t {
            fn from_file(file: std::fs::File) -> $crate::Result<Self> {
                $crate::__deps::serde_json::from_reader(std::io::BufReader::new(file))
                    .map_err($crate::Error::from)
            }

            fn to_file(&self, file: &mut std::fs::File) -> $crate::Result<()> {
                use std::io::Write;
                let mut w = std::io::BufWriter::new(file);
                $crate::__deps::serde_json::to_writer_pretty(&mut w, &self)
                    .map_err($crate::Error::from)?;
                w.flush().map_err($crate::Error::from)
            }

            fn checksum(&self) -> $crate::Result<$crate::Checksum<Self>> {
                let bytes = $crate::__deps::serde_json::to_vec_pretty(&self)?;
                Ok($crate::Checksum::new(blake3::hash(&bytes)))
            }
        }
    };
}
