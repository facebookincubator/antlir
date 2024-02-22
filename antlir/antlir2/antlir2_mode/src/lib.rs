/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! We frequently want to display mode bits in more user-friendly ways. This
//! library provides a [Mode] struct that provides direct access to the various
//! bit flags, but more importantly provides a nice symbolic representation of
//! mode strings (eg: 755 -> u+rwx,g+rx,u+rx)

use std::borrow::Cow;
use std::fmt::Debug;
use std::fmt::Display;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::str::FromStr;

use nom::branch::alt;
use nom::bytes::complete::tag;
use nom::character::complete::oct_digit1;
use nom::combinator::all_consuming;
use nom::error::convert_error;
use nom::error::ContextError;
use nom::error::ParseError;
use nom::error::VerboseError;
use nom::multi::many_m_n;
use nom::multi::separated_list0;
use nom::Finish;
use nom::IResult;

#[derive(Copy, Clone, PartialEq, Eq)]
#[repr(transparent)]
pub struct Mode(u32);

impl Mode {
    pub fn new(mode: u32) -> Self {
        Self(mode & 0o7777)
    }

    #[cfg(unix)]
    #[inline]
    pub fn as_permissions(&self) -> std::fs::Permissions {
        (*self).into()
    }

    /// Executable runs with the privileges of the owner of the file
    #[inline]
    pub fn setuid(&self) -> bool {
        (self.0 & 0o04000) != 0
    }

    /// The set-group-ID bit (S_ISGID) has several special uses. For a
    /// directory, it indicates that BSD semantics is to be used for that
    /// directory: files created there inherit their group ID from the
    /// directory, not from the effective group ID of the creating process, and
    /// directories created there will also get the S_ISGID bit set. For a file
    /// that does not have the group execution bit (S_IXGRP) set, the
    /// set-group-ID bit indicates mandatory file/record locking.
    #[inline]
    pub fn setgid(&self) -> bool {
        (self.0 & 0o02000) != 0
    }

    /// The sticky bit on a directory means that a file in that directory can be
    /// renamed or deleted only by the owner of the file, by the owner of the
    /// directory, and by a privileged process.
    #[inline]
    pub fn sticky(&self) -> bool {
        (self.0 & 0o01000) != 0
    }

    /// Permissions for the user that owns this file
    #[inline]
    pub fn user(&self) -> Permissions {
        Permissions(((self.0 >> 6) & 0b111) as u8)
    }

    /// Permissions for members of the group that owns this file
    #[inline]
    pub fn group(&self) -> Permissions {
        Permissions(((self.0 >> 3) & 0b111) as u8)
    }

    /// Permissions for everyone else
    #[inline]
    pub fn other(&self) -> Permissions {
        Permissions(((self.0) & 0b111) as u8)
    }

    fn parse<'a, E>(input: &'a str) -> IResult<&str, Self, E>
    where
        E: ParseError<&'a str> + ContextError<&'a str>,
    {
        let (input, directives) = separated_list0(tag(","), Directive::parse)(input)?;

        let mut bits = 0u32;
        for directive in directives {
            bits |= directive.mask();
        }
        Ok((input, Self(bits)))
    }
}

#[derive(Debug, Copy, Clone)]
enum Directive {
    Add {
        class: char,
        setid: bool,
        perms: Permissions,
    },
    Sticky,
}

impl Directive {
    fn parse<'a, E>(input: &'a str) -> IResult<&'a str, Self, E>
    where
        E: ParseError<&'a str> + ContextError<&'a str>,
    {
        let (input, class) = alt((tag("u"), tag("g"), tag("o"), tag("t")))(input)?;
        if class == "t" {
            return Ok((input, Directive::Sticky));
        }
        let (input, _) = tag("+")(input)?;
        let (input, (setid, perms)) = Permissions::parse(input)?;
        Ok((
            input,
            Directive::Add {
                class: class
                    .chars()
                    .next()
                    .expect("this has to be exactly one char"),
                setid,
                perms,
            },
        ))
    }

    fn mask(&self) -> u32 {
        match *self {
            Self::Add {
                class,
                setid,
                perms,
            } => match class {
                'u' => {
                    let mut bits = (perms.0 as u32) << 6;
                    if setid {
                        bits |= 0o04000;
                    }
                    bits
                }
                'g' => {
                    let mut bits = (perms.0 as u32) << 3;
                    if setid {
                        bits |= 0o02000;
                    }
                    bits
                }
                'o' => perms.0 as u32,
                _ => unreachable!(),
            },
            Self::Sticky => 0o01000,
        }
    }
}

impl Debug for Mode {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "Mode({:#o})", self.0)
    }
}

impl FromStr for Mode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        match all_consuming::<_, _, VerboseError<&str>, _>(Self::parse)(s).finish() {
            Ok((_, mode)) => Ok(mode),
            Err(e) => Err(convert_error(s, e)),
        }
    }
}

/// Bits for a single entry (user, group, other)
#[derive(Copy, Clone, PartialEq, Eq)]
#[repr(transparent)]
pub struct Permissions(u8);

impl Permissions {
    #[inline]
    pub fn read(&self) -> bool {
        (self.0 & 0b100) != 0
    }

    #[inline]
    pub fn write(&self) -> bool {
        (self.0 & 0b010) != 0
    }

    #[inline]
    pub fn execute(&self) -> bool {
        (self.0 & 0b001) != 0
    }

    fn parse<'a, E>(input: &'a str) -> IResult<&str, (bool, Self), E>
    where
        E: ParseError<&'a str> + ContextError<&'a str>,
    {
        if let Ok((input, d)) = oct_digit1::<_, (_, nom::error::ErrorKind)>(input) {
            Ok((
                input,
                (
                    false,
                    Self(u8::from_str_radix(d, 8).expect("this definitely is valid octal")),
                ),
            ))
        } else {
            let (input, symbols) =
                many_m_n(0, 3, alt((tag("r"), tag("w"), tag("x"), tag("s"))))(input)?;
            let mut bits = 0;
            let mut setid = false;
            for sym in symbols {
                bits = match sym {
                    "r" => bits | 0b100,
                    "w" => bits | 0b010,
                    "x" => bits | 0b001,
                    "s" => {
                        setid = true;
                        bits
                    }
                    _ => unreachable!("parser would have already failed"),
                }
            }
            Ok((input, (setid, Self(bits))))
        }
    }
}

impl Debug for Permissions {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.debug_tuple("Permissions")
            .field(&format!("{:#03b}", self.0))
            .finish()
    }
}

impl Display for Permissions {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        if self.read() {
            f.write_str("r")?;
        }
        if self.write() {
            f.write_str("w")?;
        }
        if self.execute() {
            f.write_str("x")?;
        }
        Ok(())
    }
}

impl Display for Mode {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let mut parts = Vec::<Cow<'_, str>>::new();
        if self.user().0 != 0 {
            parts.push(format!("u+{}{}", self.user(), if self.setuid() { "s" } else { "" }).into());
        } else if self.setuid() {
            parts.push("u+s".into());
        }
        if self.group().0 != 0 {
            parts
                .push(format!("g+{}{}", self.group(), if self.setgid() { "s" } else { "" }).into());
        } else if self.setuid() {
            parts.push("g+s".into());
        }
        if self.other().0 != 0 {
            parts.push(format!("o+{}", self.other()).into());
        }
        if self.sticky() {
            parts.push("t".into());
        }
        let mut it = parts.into_iter().peekable();
        while let Some(part) = it.next() {
            f.write_str(&part)?;
            if it.peek().is_some() {
                f.write_str(",")?;
            }
        }
        Ok(())
    }
}

impl From<u32> for Mode {
    fn from(u: u32) -> Self {
        Self::new(u)
    }
}

impl From<Mode> for u32 {
    fn from(m: Mode) -> u32 {
        m.0
    }
}

#[cfg(unix)]
impl From<Mode> for std::fs::Permissions {
    fn from(m: Mode) -> Self {
        Self::from_mode(m.0)
    }
}

#[cfg(unix)]
impl From<std::fs::Permissions> for Mode {
    fn from(p: std::fs::Permissions) -> Self {
        Self::new(p.mode())
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case(0o754, "u+rwx,g+rx,o+r")]
    #[case(0o600, "u+rw")]
    #[case(0o4111, "u+xs,g+x,o+x")]
    #[case(0o1644, "u+rw,g+r,o+r,t")]
    /// Symbolic strings can be both formatted and parsed.
    fn symbolic(#[case] bits: u32, #[case] expected: &str) {
        assert_eq!(expected, Mode::new(bits).to_string());
        assert_eq!(
            Mode::from_str(expected).expect("failed to parse"),
            Mode::new(bits)
        );
    }
}
