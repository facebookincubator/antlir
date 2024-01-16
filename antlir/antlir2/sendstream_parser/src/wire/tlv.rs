/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::time::Duration;
use std::time::SystemTime;

use nix::unistd::Gid;
use nix::unistd::Uid;
use nom::IResult;
use uuid::Uuid;

/// Parse the TLV with the output type's primary attribute tag
pub(crate) fn parse_tlv<'i, T, const L: usize, Attr>(input: &'i [u8]) -> IResult<&'i [u8], T>
where
    T: Tlv<'i, L, Attr = Attr>,
    Attr: AttrTypeParam,
{
    parse_tlv_with_attr::<'i, T, L, T::Attr>(input)
}

/// Parse the TLV with an explicit attribute tag. This allows for parsing
/// identical data types from the different Attrs.
pub(crate) fn parse_tlv_with_attr<'i, T, const L: usize, Attr>(
    input: &'i [u8],
) -> IResult<&'i [u8], T>
where
    T: Tlv<'i, L>,
    // Ensure that T can be parsed from this attribute
    T: ParsesFromAttr<Attr>,
    Attr: AttrTypeParam,
{
    // guarantee that the tlv type is what we expected
    let (input, _) = nom::bytes::streaming::tag(Attr::attr().tag())(input)?;
    match L {
        0 => {
            let (input, len) = nom::number::streaming::le_u16(input)?;
            let (input, data) = nom::bytes::streaming::take(len)(input)?;
            Ok((input, T::parse(data)))
        }
        _ => {
            // this will cause the parser to fail if the length is not
            // exactly L bytes
            let (input, _) = nom::bytes::streaming::tag((L as u16).to_le_bytes())(input)?;
            let (input, data) = nom::bytes::streaming::take(L)(input)?;
            Ok((
                input,
                #[allow(clippy::expect_used)]
                T::parse_exact(data.try_into().expect("length is already checked")),
            ))
        }
    }
}

/// Type-length-value struct. If L is not 0, the parser will automatically
/// ensure that the data is exactly L bytes long, and will call parse_exact
/// instead of parse.
pub(crate) trait Tlv<'i, const L: usize>: ParsesFromAttr<Self::Attr> {
    /// Default attribute when calling parse_tlv
    type Attr: AttrTypeParam;

    /// Parse the data into whatever the inner type is
    fn parse(_data: &'i [u8]) -> Self
    where
        Self: Sized,
    {
        unimplemented!()
    }

    fn parse_exact(_data: [u8; L]) -> Self
    where
        Self: Sized,
    {
        unimplemented!()
    }
}

pub(crate) trait ParsesFromAttr<Attr>
where
    Attr: AttrTypeParam,
{
}

macro_rules! tlv_impl {
    ($lt: lifetime, $ty: ty, $default_attr: ident, $parse: expr) => {
        impl<$lt> Tlv<$lt, 0> for $ty {
            type Attr = attr_types::$default_attr;

            fn parse(data: &$lt [u8]) -> Self {
                $parse(data)
            }
        }

        impl<$lt> ParsesFromAttr<attr_types::$default_attr> for $ty {}
    };
    ($lt: lifetime, $ty: ty, $default_attr: ident, $parse:expr, $($attr:ident),+) => {
        tlv_impl!($lt, $ty, $default_attr, $parse);
        $(impl<$lt> ParsesFromAttr<attr_types::$attr> for $ty {})+
    };
    ($ty: ty, $len: literal, $default_attr: ident, $parse: expr) => {
        impl<'i> Tlv<'i, $len> for $ty {
            type Attr = attr_types::$default_attr;

            fn parse_exact(data: [u8; $len]) -> Self {
                $parse(data)
            }
        }

        impl ParsesFromAttr<attr_types::$default_attr> for $ty {}
    };
    ($ty: ty, $len: literal, $default_attr: ident, $parse:expr, $($attr:ident),+) => {
        tlv_impl!($ty, $len, $default_attr, $parse);
        $(impl ParsesFromAttr<attr_types::$attr> for $ty {})+
    };
}

tlv_impl!(
    'i,
    &'i Path,
    Path,
    |data: &'i [u8]| -> &'i Path { Path::new(OsStr::from_bytes(data)) },
    PathTo,
    ClonePath
);

tlv_impl!(
    'i,
    crate::TemporaryPath<'i>,
    Path,
    |data: &'i [u8]| -> crate::TemporaryPath<'i> { crate::TemporaryPath(Path::new(OsStr::from_bytes(data))) }
);

tlv_impl!(
    Uuid,
    16,
    Uuid,
    |data: [u8; 16]| -> Uuid { Uuid::from_u128_le(u128::from_le_bytes(data)) },
    CloneUuid
);

tlv_impl!(
    crate::Ctransid,
    8,
    Ctransid,
    |data: [u8; 8]| -> crate::Ctransid { crate::Ctransid(u64::from_le_bytes(data)) },
    CloneCtransid
);

tlv_impl!(Uid, 8, Uid, |data: [u8; 8]| -> Uid {
    Uid::from_raw(u64::from_le_bytes(data) as u32)
});

tlv_impl!(Gid, 8, Gid, |data: [u8; 8]| -> Gid {
    Gid::from_raw(u64::from_le_bytes(data) as u32)
});

tlv_impl!(crate::Mode, 8, Mode, |data: [u8; 8]| -> crate::Mode {
    crate::Mode(u64::from_le_bytes(data) as u32)
});

tlv_impl!(crate::Ino, 8, Ino, |data: [u8; 8]| -> crate::Ino {
    crate::Ino(u64::from_le_bytes(data))
});

tlv_impl!(
    'i,
    crate::XattrName<'i>,
    XattrName,
    |data: &'i [u8]| -> crate::XattrName<'i> {
        crate::XattrName(data)
    }
);

tlv_impl!(
    'i,
    crate::XattrData<'i>,
    XattrData,
    |data: &'i [u8]| -> crate::XattrData<'i> {
        crate::XattrData(data)
    }
);

tlv_impl!(
    crate::FileOffset,
    8,
    FileOffset,
    |data: [u8; 8]| -> crate::FileOffset { crate::FileOffset(u64::from_le_bytes(data)) },
    CloneOffset
);

tlv_impl!(
    'i,
    crate::Data<'i>,
    Data,
    |data: &'i [u8]| -> crate::Data<'i> {
        crate::Data(data)
    }
);

tlv_impl!(
    'i,
    crate::LinkTarget<'i>,
    Link,
    |data: &'i [u8]| -> crate::LinkTarget<'i> {
        crate::LinkTarget(Path::new(OsStr::from_bytes(data)))
    }
);

tlv_impl!(crate::Rdev, 8, Rdev, |data: [u8; 8]| -> crate::Rdev {
    crate::Rdev(u64::from_le_bytes(data))
});

tlv_impl!(
    crate::CloneLen,
    8,
    CloneLen,
    |data: [u8; 8]| -> crate::CloneLen { crate::CloneLen(u64::from_le_bytes(data)) }
);

tlv_impl!(u64, 8, Size, |data: [u8; 8]| -> u64 {
    u64::from_le_bytes(data)
});

fn parse_time(data: [u8; 12]) -> SystemTime {
    #[allow(clippy::expect_used)]
    let secs = u64::from_le_bytes(data[..8].try_into().expect("right size"));
    #[allow(clippy::expect_used)]
    let nanos = u32::from_le_bytes(data[8..].try_into().expect("right size"));
    SystemTime::UNIX_EPOCH + Duration::from_secs(secs) + Duration::from_nanos(nanos.into())
}

macro_rules! time_tlv {
    ($i:ident) => {
        tlv_impl!(crate::$i, 12, $i, |data: [u8; 12]| -> crate::$i {
            crate::$i(parse_time(data))
        });
    };
}

time_tlv!(Atime);
time_tlv!(Mtime);
time_tlv!(Ctime);

pub(crate) trait AttrTypeParam {
    fn attr() -> Attr;
}

macro_rules! gen_attrs_code {
    ($enm: ident, $($v:ident),+) => {
        #[derive(
            Debug,
            Copy,
            Clone,
            PartialEq,
            Eq,
            PartialOrd,
            Ord,
        )]
        pub(crate) enum $enm {
            $($v,)+
        }

        impl $enm {
            const fn as_u16(self) -> u16 {
                match self {
                    $(Self::$v => ${index()},)+
                }
            }
        }

        pub(crate) mod attr_types {
            /// Empty type used as type parameter for parse_tlv
            $(
                pub(crate) struct $v();
                impl super::AttrTypeParam for $v {
                    fn attr() -> super::Attr {
                        super::Attr::$v
                    }
                }
            )+
        }
    }
}

gen_attrs_code!(
    Attr,
    // variants go below, order is important and must match send.h
    Unspecified,
    Uuid,
    Ctransid,
    Ino,
    Size,
    Mode,
    Uid,
    Gid,
    Rdev,
    Ctime,
    Mtime,
    Atime,
    Otime,
    XattrName,
    XattrData,
    Path,
    PathTo,
    Link,
    FileOffset,
    Data,
    CloneUuid,
    CloneCtransid,
    ClonePath,
    CloneOffset,
    CloneLen
);

impl Attr {
    fn tag(self) -> [u8; 2] {
        self.as_u16().to_le_bytes()
    }
}
