/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;
use serde::Serializer;

use crate::Data;
use crate::XattrData;
use crate::XattrName;

pub(crate) mod uid {
    use nix::unistd::Uid;

    use super::*;

    pub fn deserialize<'de, D>(d: D) -> Result<Uid, D::Error>
    where
        D: Deserializer<'de>,
    {
        u32::deserialize(d).map(Uid::from_raw)
    }

    pub fn serialize<S>(u: &Uid, s: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        u.as_raw().serialize(s)
    }
}

pub(crate) mod gid {
    use nix::unistd::Gid;

    use super::*;

    pub fn deserialize<'de, D>(d: D) -> Result<Gid, D::Error>
    where
        D: Deserializer<'de>,
    {
        u32::deserialize(d).map(Gid::from_raw)
    }

    pub fn serialize<S>(g: &Gid, s: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        g.as_raw().serialize(s)
    }
}

pub(crate) mod utf8 {
    use serde::ser::Error;

    use super::*;

    pub fn deserialize<'de, D, T>(d: D) -> Result<T, D::Error>
    where
        D: Deserializer<'de>,
        T: From<&'de [u8]>,
    {
        <&str>::deserialize(d).map(|s| s.as_bytes()).map(T::from)
    }

    pub fn serialize<S, T>(t: T, s: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
        T: AsRef<[u8]>,
    {
        std::str::from_utf8(t.as_ref())
            .map_err(|_| S::Error::custom("not utf8 string"))
            .and_then(|d| d.serialize(s))
    }
}

macro_rules! utf8_serde {
    ($t:ident) => {
        impl<'a, 'de> Deserialize<'de> for $t<'a>
        where
            'de: 'a,
        {
            fn deserialize<D>(d: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                crate::ser::utf8::deserialize(d)
            }
        }

        impl<'a> Serialize for $t<'a> {
            fn serialize<S>(&self, s: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                crate::ser::utf8::serialize(self, s)
            }
        }
    };
}

utf8_serde!(Data);
utf8_serde!(XattrName);
utf8_serde!(XattrData);
