/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! dm-verity v1 hash-tree creation, entirely in userspace.
//!
//! Layout of the combined image produced by [`format_image`]:
//! `[ data blocks, zero-padded to data_block_size ]`
//! `[ verity superblock (1 hash block)            ]` <- hash_offset
//! `[ hash tree top level (1 block, the root block)]`
//! `[ ... intermediate levels                     ]`
//! `[ hash tree level 0 blocks (data hashes)      ]`
//!
//! Hash blocks are stored top-first, as the kernel and cryptsetup expect:
//! `hash_start` points at the root block, which is the first hash block
//! after the superblock.
//!
//! Hashing follows cryptsetup's `verity_hash.c` for hash_type 1: each block
//! is hashed as `HASH(salt || block)`, hashes are packed digest-size-aligned
//! into hash blocks, and each higher level hashes the hash blocks of the
//! level below. The root hash is `HASH(salt || top_level_hash_block)`.

use std::fs::File;
use std::io::BufReader;
use std::io::BufWriter;
use std::io::Read;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;
use serde::Deserialize;
use sha2::Digest;
use sha2::Sha256;
use sha2::Sha512;
use uuid::Uuid;

const SUPERBLOCK_SIZE: usize = 512;
const SIGNATURE: &[u8; 8] = b"verity\0\0";
const MAX_SALT_SIZE: usize = 256;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DmVerity {
    src: PathBuf,
    data_block_size: u32,
    hash_block_size: u32,
    hash_algorithm: HashAlgorithm,
    salt: String,
    uuid: Uuid,
    hash_image: PathBuf,
    root_hash: PathBuf,
}

impl DmVerity {
    pub(crate) fn build(&self, out: &Path) -> anyhow::Result<()> {
        let config = Config {
            data_block_size: self.data_block_size,
            hash_block_size: self.hash_block_size,
            hash_algorithm: self.hash_algorithm,
            salt: hex::decode(&self.salt).context("invalid salt hex")?,
            uuid: self.uuid,
        };
        format_image(
            &self.src,
            out,
            &self.root_hash,
            Some(&self.hash_image),
            &config,
        )?;
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
enum DmVerityError {
    #[error("invalid block size {0}: must be one of 512, 1024, 2048, 4096")]
    InvalidBlockSize(u32),
    #[error("salt too long: {0} bytes (max {MAX_SALT_SIZE})")]
    SaltTooLong(usize),
    #[error("data image is empty")]
    EmptyData,
    #[error("hash block too small for digest")]
    HashBlockTooSmall,
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

type Result<T> = std::result::Result<T, DmVerityError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, strum::Display)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
enum HashAlgorithm {
    Sha256,
    Sha512,
}

impl HashAlgorithm {
    fn digest_size(&self) -> usize {
        match self {
            Self::Sha256 => 32,
            Self::Sha512 => 64,
        }
    }

    fn hash(&self, salt: &[u8], data: &[u8]) -> Vec<u8> {
        match self {
            Self::Sha256 => {
                let mut h = Sha256::new();
                h.update(salt);
                h.update(data);
                h.finalize().to_vec()
            }
            Self::Sha512 => {
                let mut h = Sha512::new();
                h.update(salt);
                h.update(data);
                h.finalize().to_vec()
            }
        }
    }
}

#[derive(Debug, Clone)]
struct Config {
    data_block_size: u32,
    hash_block_size: u32,
    hash_algorithm: HashAlgorithm,
    salt: Vec<u8>,
    uuid: Uuid,
}

fn validate_block_size(size: u32) -> Result<()> {
    match size {
        512 | 1024 | 2048 | 4096 => Ok(()),
        other => Err(DmVerityError::InvalidBlockSize(other)),
    }
}

fn digest_full_size(digest_size: usize) -> usize {
    let mut size = 1usize;
    while size < digest_size {
        size <<= 1;
    }
    size
}

fn hashes_per_block(hash_block_size: u32, digest_size: usize) -> usize {
    let per = (hash_block_size as usize) / digest_full_size(digest_size);
    let mut p = 1usize;
    while p * 2 <= per {
        p *= 2;
    }
    p
}

fn pack_hash_block(hashes: &[Vec<u8>], hash_block_size: u32, digest_size: usize) -> Vec<u8> {
    let full = digest_full_size(digest_size);
    let mut block = vec![0u8; hash_block_size as usize];
    for (i, digest) in hashes.iter().enumerate() {
        block[i * full..i * full + digest_size].copy_from_slice(digest);
    }
    block
}

/// Build every hash-tree level (level 0 first) and the root hash from the
/// hashes of the level below, starting from data-block hashes.
fn build_levels(
    first_hashes: Vec<Vec<u8>>,
    config: &Config,
) -> Result<(Vec<Vec<Vec<u8>>>, Vec<u8>)> {
    let digest_size = config.hash_algorithm.digest_size();
    let per_block = hashes_per_block(config.hash_block_size, digest_size);
    if per_block < 1 {
        return Err(DmVerityError::HashBlockTooSmall);
    }
    let mut levels = Vec::new();
    let mut current = first_hashes;
    loop {
        let level_blocks: Vec<Vec<u8>> = current
            .chunks(per_block)
            .map(|chunk| pack_hash_block(chunk, config.hash_block_size, digest_size))
            .collect();
        let is_top = level_blocks.len() <= 1;
        let root = if is_top {
            Some(config.hash_algorithm.hash(&config.salt, &level_blocks[0]))
        } else {
            None
        };
        current = level_blocks
            .iter()
            .map(|block| config.hash_algorithm.hash(&config.salt, block))
            .collect();
        levels.push(level_blocks);
        if let Some(root_hash) = root {
            return Ok((levels, root_hash));
        }
    }
}

fn superblock(config: &Config, data_blocks: u64) -> [u8; SUPERBLOCK_SIZE] {
    let mut sb = [0u8; SUPERBLOCK_SIZE];
    sb[0..8].copy_from_slice(SIGNATURE);
    sb[8..12].copy_from_slice(&1u32.to_le_bytes());
    sb[12..16].copy_from_slice(&1u32.to_le_bytes());
    sb[16..32].copy_from_slice(config.uuid.as_bytes());
    let algo = config.hash_algorithm.to_string();
    sb[32..32 + algo.len()].copy_from_slice(algo.as_bytes());
    sb[64..68].copy_from_slice(&config.data_block_size.to_le_bytes());
    sb[68..72].copy_from_slice(&config.hash_block_size.to_le_bytes());
    sb[72..80].copy_from_slice(&data_blocks.to_le_bytes());
    sb[80..82].copy_from_slice(&(config.salt.len() as u16).to_le_bytes());
    sb[88..88 + config.salt.len()].copy_from_slice(&config.salt);
    sb
}

/// Format `data_path` into a combined dm-verity image at `out_path`, writing
/// the hex root hash to `root_hash_path`. If `hash_path` is given, also write
/// the hash area on its own there (superblock block followed by the hash
/// tree, top-first) — this is the image used for a separate verity hash
/// partition in the systemd Discoverable Partitions layout. Returns the
/// root hash bytes.
fn format_image(
    data_path: &Path,
    out_path: &Path,
    root_hash_path: &Path,
    hash_path: Option<&Path>,
    config: &Config,
) -> Result<Vec<u8>> {
    validate_block_size(config.data_block_size)?;
    validate_block_size(config.hash_block_size)?;
    if config.salt.len() > MAX_SALT_SIZE {
        return Err(DmVerityError::SaltTooLong(config.salt.len()));
    }

    let data_file = File::open(data_path)?;
    let data_len = data_file.metadata()?.len();
    if data_len == 0 {
        return Err(DmVerityError::EmptyData);
    }
    let block = config.data_block_size as u64;
    let data_blocks = data_len.div_ceil(block);

    let mut reader = BufReader::with_capacity(1 << 20, data_file);
    let mut out = BufWriter::with_capacity(1 << 20, File::create(out_path)?);
    let mut first_hashes = Vec::with_capacity(data_blocks as usize);
    let mut buf = vec![0u8; config.data_block_size as usize];
    let mut remaining = data_len;
    while remaining > 0 {
        let to_read = std::cmp::min(remaining, block) as usize;
        reader.read_exact(&mut buf[..to_read])?;
        for byte in &mut buf[to_read..] {
            *byte = 0;
        }
        out.write_all(&buf)?;
        first_hashes.push(config.hash_algorithm.hash(&config.salt, &buf));
        remaining -= to_read as u64;
    }

    let (levels, root_hash) = build_levels(first_hashes, config)?;

    let sb = superblock(config, data_blocks);
    let mut sb_block = vec![0u8; config.hash_block_size as usize];
    sb_block[..SUPERBLOCK_SIZE].copy_from_slice(&sb);
    // Stored top-first: the root block is the first hash block after the
    // superblock, so `hash_start` (hash_offset / hash_block_size + 1) points
    // at it, matching the kernel dm-verity and cryptsetup layouts.
    let mut hash_image = sb_block;
    for level in levels.iter().rev() {
        for hash_block in level {
            hash_image.extend_from_slice(hash_block);
        }
    }
    out.write_all(&hash_image)?;
    out.flush()?;
    if let Some(hash_path) = hash_path {
        std::fs::write(hash_path, &hash_image)?;
    }

    std::fs::write(root_hash_path, format!("{}\n", hex::encode(&root_hash)))?;
    Ok(root_hash)
}
