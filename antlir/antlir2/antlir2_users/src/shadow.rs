/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Parse `/etc/shadow` file to get a map of all the users inside of an
//! under-construction image so that ownership is attributed properly.

use std::borrow::Cow;
use std::fmt::Display;
use std::fmt::Formatter;
use std::str::FromStr;

use nom::bytes::complete::take_until;
use nom::bytes::complete::take_until1;
use nom::character::complete::char;
use nom::character::complete::newline;
use nom::combinator::all_consuming;
use nom::error::context;
use nom::error::convert_error;
use nom::error::ContextError;
use nom::error::ParseError;
use nom::error::VerboseError;
use nom::multi::many0;
use nom::multi::separated_list0;
use nom::sequence::tuple;
use nom::Finish;
use nom::IResult;

use crate::Error;
use crate::Result;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EtcShadow<'a> {
    records: Vec<ShadowRecord<'a>>,
}

impl<'a> EtcShadow<'a> {
    fn parse_internal<E>(input: &'a str) -> IResult<&'a str, Self, E>
    where
        E: ParseError<&'a str> + ContextError<&'a str>,
    {
        let (input, records) =
            separated_list0(newline, context("ShadowRecord", ShadowRecord::parse))(input)?;
        // eat any trailing newlines
        let (input, _) = all_consuming(many0(newline))(input)?;
        Ok((input, Self { records }))
    }

    pub fn parse(input: &'a str) -> Result<Self> {
        Self::parse_internal::<VerboseError<&str>>(input)
            .finish()
            .map(|(_input, s)| s)
            .map_err(|e| Error::Parse(convert_error(input, e)))
    }

    pub fn new() -> Self {
        Default::default()
    }

    pub fn records(&self) -> impl Iterator<Item = &ShadowRecord<'a>> {
        self.records.iter()
    }

    pub fn into_records(self) -> impl Iterator<Item = ShadowRecord<'a>> {
        self.records.into_iter()
    }

    pub fn push(&mut self, record: ShadowRecord<'a>) {
        self.records.push(record)
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    pub fn into_owned(self) -> EtcShadow<'static> {
        EtcShadow {
            records: self
                .records
                .into_iter()
                .map(ShadowRecord::into_owned)
                .collect(),
        }
    }
}

impl FromStr for EtcShadow<'static> {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        let s = EtcShadow::parse(s)?;
        Ok(s.into_owned())
    }
}

impl<'a> Display for EtcShadow<'a> {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        for record in &self.records {
            writeln!(f, "{}", record)?;
        }
        Ok(())
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Days(pub u32);

impl AsRef<u32> for Days {
    fn as_ref(&self) -> &u32 {
        &self.0
    }
}

impl Days {
    pub fn as_u32(&self) -> u32 {
        self.0
    }

    fn parse<'a, E>(input: &'a str) -> IResult<&'a str, Self, E>
    where
        E: ParseError<&'a str> + ContextError<&'a str>,
    {
        let (input, days) = nom::character::complete::u32(input)?;
        Ok((input, Self(days)))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ShadowRecord<'a> {
    pub name: Cow<'a, str>,
    pub encrypted_password: Cow<'a, str>,
    pub last_password_change: Option<Days>,
    pub minimum_password_age: Option<Days>,
    pub maximum_password_age: Option<Days>,
    pub password_warning_period: Option<Days>,
    pub password_inactivity_period: Option<Days>,
    pub account_expiration_date: Option<Days>,
    pub reserved: Cow<'a, str>,
}

impl<'a> ShadowRecord<'a> {
    fn parse<E>(input: &'a str) -> IResult<&'a str, Self, E>
    where
        E: ParseError<&'a str> + ContextError<&'a str>,
    {
        let colon = char(':');
        let (
            input,
            (
                name,
                _,
                encrypted_password,
                _,
                last_password_change,
                _,
                minimum_password_age,
                _,
                maximum_password_age,
                _,
                password_warning_period,
                _,
                password_inactivity_period,
                _,
                account_expiration_date,
                _,
                reserved,
            ),
        ) = tuple((
            context("username", take_until1(":")),
            &colon,
            context("encrypted_password", take_until1(":")),
            &colon,
            context("last_password_change", nom::combinator::opt(Days::parse)),
            &colon,
            context("minimum_password_age", nom::combinator::opt(Days::parse)),
            &colon,
            context("maximum_password_age", nom::combinator::opt(Days::parse)),
            &colon,
            context("password_warning_period", nom::combinator::opt(Days::parse)),
            &colon,
            context(
                "password_inactivity_period",
                nom::combinator::opt(Days::parse),
            ),
            &colon,
            context("account_expiration_date", nom::combinator::opt(Days::parse)),
            &colon,
            context("reserved", take_until("\n")),
        ))(input)?;
        Ok((
            input,
            Self {
                name: Cow::Borrowed(name),
                encrypted_password: Cow::Borrowed(encrypted_password),
                last_password_change,
                minimum_password_age,
                maximum_password_age,
                password_warning_period,
                password_inactivity_period,
                account_expiration_date,
                reserved: Cow::Borrowed(reserved),
            },
        ))
    }

    pub fn into_owned(self) -> ShadowRecord<'static> {
        ShadowRecord {
            name: Cow::Owned(self.name.into_owned()),
            encrypted_password: Cow::Owned(self.encrypted_password.into_owned()),
            last_password_change: self.last_password_change.clone(),
            minimum_password_age: self.minimum_password_age.clone(),
            maximum_password_age: self.maximum_password_age.clone(),
            password_warning_period: self.password_warning_period.clone(),
            password_inactivity_period: self.password_inactivity_period.clone(),
            account_expiration_date: self.account_expiration_date.clone(),
            reserved: Cow::Owned(self.reserved.into_owned()),
        }
    }
}

struct OptionalDays<'a>(&'a Option<Days>);

impl<'a> Display for OptionalDays<'a> {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        match self.0 {
            Some(days) => write!(f, "{}", days.0),
            None => Ok(()),
        }
    }
}

impl<'a> Display for ShadowRecord<'a> {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        let Self {
            name,
            encrypted_password,
            last_password_change,
            minimum_password_age,
            maximum_password_age,
            password_warning_period,
            password_inactivity_period,
            account_expiration_date,
            reserved,
        } = self;
        write!(
            f,
            "{name}:{encrypted_password}:{last_password_change}:{minimum_password_age}:{maximum_password_age}:{password_warning_period}:{password_inactivity_period}:{account_expiration_date}:{reserved}",
            last_password_change = OptionalDays(last_password_change),
            minimum_password_age = OptionalDays(minimum_password_age),
            maximum_password_age = OptionalDays(maximum_password_age),
            password_warning_period = OptionalDays(password_warning_period),
            password_inactivity_period = OptionalDays(password_inactivity_period),
            account_expiration_date = OptionalDays(account_expiration_date),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_etc_shadow() {
        let src = r#"bin:*:18397:0:99999:7:::
"#;
        let shadow = EtcShadow::parse(src).expect("failed to parse");
        // if Display matches the src, we haven't lost any information
        assert_eq!(shadow.to_string(), src);
    }
}
