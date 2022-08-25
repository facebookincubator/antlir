/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::num::NonZeroU32;

use anyhow::Context;
use anyhow::Result;
use byte_unit::Byte;
use clap::Parser as ClapParser;
use governor::clock::DefaultClock;
use governor::state::InMemoryState;
use governor::state::NotKeyed;
use governor::Quota;
use strum_macros::EnumString;
#[derive(Debug, Eq, PartialEq, EnumString, Clone)]
#[strum(serialize_all = "lowercase")]
pub enum Architecture {
    All,
    Amd64,
    Arm64,
    Armel,
    Armhf,
    I386,
    Mips64el,
    Ppc64el,
    S390x,
    #[strum(default)]
    Unknown(String),
}

#[derive(ClapParser, Debug)]
pub struct Args {
    #[clap(long)]
    pub repourl: String,
    #[clap(long)]
    pub distro: String,
    #[clap(long)]
    pub flavor: String,
    #[clap(long)]
    pub arch: String,
    #[clap(long, parse(try_from_str=parse_qps))]
    pub readqps: Option<governor::RateLimiter<NotKeyed, InMemoryState, DefaultClock>>,
    #[clap(long, parse(try_from_str=parse_qps))]
    pub writeqps: Option<governor::RateLimiter<NotKeyed, InMemoryState, DefaultClock>>,
    ///max throughput of write to storage
    #[clap(long, parse(try_from_str=parse_throughput))]
    pub writethroughput: Option<governor::RateLimiter<NotKeyed, InMemoryState, DefaultClock>>,
}

fn parse_qps(qps: &str) -> Result<governor::RateLimiter<NotKeyed, InMemoryState, DefaultClock>> {
    Ok(governor::RateLimiter::direct(Quota::per_second(
        NonZeroU32::new(qps.parse()?)
            .context("qps cannot be zero - omit option for no ratelimit")?,
    )))
}

fn parse_throughput(
    throughput: &str,
) -> Result<governor::RateLimiter<NotKeyed, InMemoryState, DefaultClock>> {
    Ok(governor::RateLimiter::direct(Quota::per_second(
        NonZeroU32::new(Byte::from_str(throughput)?.get_bytes().try_into()?)
            .context("throughput cannot be zero - omit option for no ratelimit")?,
    )))
}
