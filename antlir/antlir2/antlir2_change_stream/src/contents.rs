/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::io::BufReader;
use std::io::Read;

pub trait Contents: Sized + Send {
    fn from_file(file: std::fs::File) -> std::io::Result<Self>;

    fn differs(&mut self, other: &mut Self) -> std::io::Result<bool>;
}

impl Contents for Vec<u8> {
    fn from_file(file: std::fs::File) -> std::io::Result<Self> {
        let mut br = BufReader::new(file);
        let mut buf = Vec::new();
        br.read_to_end(&mut buf)?;
        Ok(buf)
    }

    fn differs(&mut self, other: &mut Self) -> std::io::Result<bool> {
        Ok(self != other)
    }
}

impl Contents for std::fs::File {
    fn from_file(file: std::fs::File) -> std::io::Result<Self> {
        Ok(file)
    }

    fn differs(&mut self, other: &mut Self) -> std::io::Result<bool> {
        // if the length differs, the files must be different
        if self.metadata()?.len() != other.metadata()?.len() {
            return Ok(true);
        }
        readers_differ(BufReader::new(self), BufReader::new(other))
    }
}

impl Contents for BufReader<std::fs::File> {
    fn from_file(file: std::fs::File) -> std::io::Result<Self> {
        Ok(BufReader::new(file))
    }

    fn differs(&mut self, other: &mut Self) -> std::io::Result<bool> {
        // if the length differs, the files must be different
        if self.get_ref().metadata()?.len() != other.get_ref().metadata()?.len() {
            return Ok(true);
        }
        readers_differ(self, other)
    }
}

fn readers_differ(mut a: impl Read, mut b: impl Read) -> std::io::Result<bool> {
    let chunk_size = 0x4000;
    loop {
        let mut a_buf = Vec::with_capacity(chunk_size);
        let mut b_buf = Vec::with_capacity(chunk_size);
        let a_n = a.by_ref().take(chunk_size as u64).read_to_end(&mut a_buf)?;
        // no need to count how many bytes was read from b, since the entire
        // chunk contents will be checked right after this
        b.by_ref().take(chunk_size as u64).read_to_end(&mut b_buf)?;
        // if at any point a chunk is not equal, the files are different and
        // we don't need to look at the rest
        if a_buf != b_buf {
            return Ok(true);
        }
        if a_n == 0 || a_n < chunk_size {
            // reached EOF without finding a difference
            return Ok(false);
        }
    }
}
