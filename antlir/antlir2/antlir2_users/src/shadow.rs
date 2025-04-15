/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Parse `/etc/shadow` file to get a map of all the users inside of an
//! under-construction image so that ownership is attributed properly.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fmt::Display;
use std::fmt::Formatter;
use std::str::FromStr;

use maplit::btreemap;
use nom::Finish;
use nom::IResult;
use nom::bytes::complete::take_until;
use nom::bytes::complete::take_until1;
use nom::character::complete::char;
use nom::character::complete::newline;
use nom::combinator::all_consuming;
use nom::error::ContextError;
use nom::error::ParseError;
use nom::error::VerboseError;
use nom::error::context;
use nom::error::convert_error;
use nom::multi::many0;
use nom::multi::separated_list0;
use nom::sequence::tuple;

use crate::Error;
use crate::Result;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EtcShadow<'a> {
    records: Vec<ShadowRecord<'a>>,
    username_to_record_idx: BTreeMap<String, usize>,
}

impl<'a> Default for EtcShadow<'a> {
    fn default() -> Self {
        Self {
            records: vec![ShadowRecord {
                name: "root".into(),
                encrypted_password: ShadowRecordPassword::NoLoginButPermitSSH,
                last_password_change: None,
                minimum_password_age: None,
                maximum_password_age: None,
                password_warning_period: None,
                password_inactivity_period: None,
                account_expiration_date: None,
                reserved: "".into(),
            }],
            username_to_record_idx: btreemap! {"root".to_string() => 0},
        }
    }
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
        Ok((
            input,
            Self {
                username_to_record_idx: records
                    .iter()
                    .enumerate()
                    .map(|(idx, r)| (r.name.to_string(), idx))
                    .collect(),
                records,
            },
        ))
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

    pub fn push(&mut self, record: ShadowRecord<'a>) -> Result<()> {
        match self.get_user_shadow_by_name(&record.name) {
            Some(existing) if *existing == record => Ok(()),
            Some(existing) => Err(Error::Duplicate(
                existing.name.to_string(),
                format!("{:?}", existing),
                format!("{:?}", record),
            )),
            None => {
                self.username_to_record_idx
                    .insert(record.name.to_string(), self.records.len());
                self.records.push(record);
                Ok(())
            }
        }
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    pub fn get_user_shadow_by_name(&self, name: &str) -> Option<&ShadowRecord<'a>> {
        self.username_to_record_idx
            .get(name)
            .and_then(|&idx| self.records.get(idx))
    }

    pub fn into_owned(self) -> EtcShadow<'static> {
        EtcShadow {
            records: self
                .records
                .into_iter()
                .map(ShadowRecord::into_owned)
                .collect(),
            username_to_record_idx: self.username_to_record_idx,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShadowRecordPassword {
    /// Login by password is disabled (!, !!, !*).
    /// OpenSSH in `UsePAM no` mode will not allow logging in.
    /// OpenSSH in `UsePAM yes` mode may allow logging in.
    NoLogin,
    /// Login by password is disabled (*).
    /// OpenSSH may allow logging in.
    NoLoginButPermitSSH,
    /// Login is enabled, and no password is required.
    OpenLogin,
    /// Login is enabled, and a password is required.
    /// The shadow record contains the hash of the password.
    EncryptedPassword(String),
}

impl From<&str> for ShadowRecordPassword {
    fn from(s: &str) -> Self {
        match s {
            "!" | "!*" | "!!" => Self::NoLogin,
            "*" => Self::NoLoginButPermitSSH,
            "" => Self::OpenLogin,
            _ => Self::EncryptedPassword(s.to_string()),
        }
    }
}

impl Display for ShadowRecordPassword {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        match self {
            Self::NoLogin => "!!".fmt(f),
            Self::NoLoginButPermitSSH => "*".fmt(f),
            Self::OpenLogin => "".fmt(f),
            Self::EncryptedPassword(s) => s.fmt(f),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ShadowRecord<'a> {
    pub name: Cow<'a, str>,
    pub encrypted_password: ShadowRecordPassword,
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
            context("encrypted_password", take_until(":")),
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
                encrypted_password: encrypted_password.into(),
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
            encrypted_password: self.encrypted_password,
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

impl<'a> PartialEq for ShadowRecord<'a> {
    fn eq(&self, other: &Self) -> bool {
        let Self {
            name: self_name,
            encrypted_password: self_encrypted_password,
            last_password_change: self_last_password_change,
            minimum_password_age: self_minimum_password_age,
            maximum_password_age: self_maximum_password_age,
            password_warning_period: self_password_warning_period,
            password_inactivity_period: self_password_inactivity_period,
            account_expiration_date: self_account_expiration_date,
            reserved: self_reserved,
        } = self;
        let Self {
            name: other_name,
            encrypted_password: other_encrypted_password,
            last_password_change: other_last_password_change,
            minimum_password_age: other_minimum_password_age,
            maximum_password_age: other_maximum_password_age,
            password_warning_period: other_password_warning_period,
            password_inactivity_period: other_password_inactivity_period,
            account_expiration_date: other_account_expiration_date,
            reserved: other_reserved,
        } = other;

        // Compare the regular fields.
        if self_name != other_name
            || self_encrypted_password != other_encrypted_password
            || self_reserved != other_reserved
        {
            return false;
        }

        // If a password is set, compare the password rotation rules.
        if let ShadowRecordPassword::EncryptedPassword(_) = self_encrypted_password {
            if self_last_password_change != other_last_password_change
                || self_minimum_password_age != other_minimum_password_age
                || self_maximum_password_age != other_maximum_password_age
                || self_password_warning_period != other_password_warning_period
                || self_password_inactivity_period != other_password_inactivity_period
                || self_account_expiration_date != other_account_expiration_date
            {
                return false;
            }
        }

        true
    }
}

impl<'a> Eq for ShadowRecord<'a> {}

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
        let src = r#"root::19760:0:99999:7:::
bin:*:18397:0:99999:7:::
"#;
        let shadow = EtcShadow::parse(src).expect("failed to parse");
        // if Display matches the src, we haven't lost any information
        assert_eq!(shadow.to_string(), src);
    }
}
