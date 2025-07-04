/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Parse `/etc/group` files to get a map of all the groups inside of an
//! under-construction image so that ownership is attributed properly.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fmt::Display;
use std::fmt::Formatter;
use std::str::FromStr;

use maplit::btreemap;
use nom::Finish;
use nom::IResult;
use nom::Parser as _;
use nom::bytes::complete::take_until;
use nom::bytes::complete::take_until1;
use nom::character::complete::char;
use nom::character::complete::newline;
use nom::character::complete::none_of;
use nom::combinator::all_consuming;
use nom::combinator::recognize;
use nom::error::ContextError;
use nom::error::ParseError;
use nom::error::context;
use nom::multi::many0;
use nom::multi::many1;
use nom::multi::separated_list0;
use nom_language::error::VerboseError;
use nom_language::error::convert_error;

use crate::Error;
use crate::GroupId;
use crate::Id;
use crate::Result;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EtcGroup<'a> {
    records: Vec<GroupRecord<'a>>,
    gid_to_record_idx: BTreeMap<GroupId, usize>,
    groupname_to_record_idx: BTreeMap<String, usize>,
}

impl<'a> Default for EtcGroup<'a> {
    fn default() -> Self {
        Self {
            records: vec![GroupRecord {
                name: "root".into(),
                password: GroupRecordPassword::X,
                gid: GroupId(0),
                users: vec!["root".into()],
            }],
            gid_to_record_idx: btreemap! {GroupId(0) => 0},
            groupname_to_record_idx: btreemap! {"root".to_string() => 0},
        }
    }
}

impl<'a> EtcGroup<'a> {
    fn parse_internal<E>(input: &'a str) -> IResult<&'a str, Self, E>
    where
        E: ParseError<&'a str> + ContextError<&'a str>,
    {
        let (input, records) =
            separated_list0(newline, context("GroupRecord", GroupRecord::parse)).parse(input)?;
        // eat trailing newlines
        let (input, _) = all_consuming(many0(newline)).parse(input)?;
        Ok((
            input,
            Self {
                gid_to_record_idx: records
                    .iter()
                    .enumerate()
                    .map(|(idx, r)| (r.gid, idx))
                    .collect(),
                groupname_to_record_idx: records
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

    pub fn records(&self) -> impl Iterator<Item = &GroupRecord<'a>> {
        self.records.iter()
    }

    pub fn into_records(self) -> impl Iterator<Item = GroupRecord<'a>> {
        self.records.into_iter()
    }

    pub fn push(&mut self, record: GroupRecord<'a>) -> Result<()> {
        match (
            self.get_group_by_id(record.gid),
            self.get_group_by_name(&record.name),
        ) {
            (Some(existing), _) | (_, Some(existing)) if *existing == record => Ok(()),
            (Some(existing), _) | (_, Some(existing)) => Err(Error::Duplicate(
                existing.name.to_string(),
                format!("{:?}", existing),
                format!("{:?}", record),
            )),
            (None, None) => {
                self.gid_to_record_idx
                    .insert(record.gid, self.records.len());
                self.groupname_to_record_idx
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

    pub fn get_group_by_name(&self, name: &str) -> Option<&GroupRecord<'a>> {
        self.groupname_to_record_idx
            .get(name)
            .and_then(|&idx| self.records.get(idx))
    }

    pub fn get_group_by_name_mut(&mut self, name: &str) -> Option<&mut GroupRecord<'a>> {
        self.groupname_to_record_idx
            .get(name)
            .and_then(|&idx| self.records.get_mut(idx))
    }

    pub fn get_group_by_id(&self, id: GroupId) -> Option<&GroupRecord<'a>> {
        self.gid_to_record_idx
            .get(&id)
            .and_then(|&idx| self.records.get(idx))
    }

    pub fn into_owned(self) -> EtcGroup<'static> {
        EtcGroup {
            records: self
                .records
                .into_iter()
                .map(GroupRecord::into_owned)
                .collect(),
            gid_to_record_idx: self.gid_to_record_idx,
            groupname_to_record_idx: self.groupname_to_record_idx,
        }
    }
}

impl FromStr for EtcGroup<'static> {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        let s = EtcGroup::parse(s)?;
        Ok(s.into_owned())
    }
}

impl<'a> Display for EtcGroup<'a> {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        for record in &self.records {
            writeln!(f, "{}", record)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GroupRecordPassword {
    /// The group password field is always set
    /// to "x" on modern Unix systems.
    X,
}

impl GroupRecordPassword {
    fn parse<'a, E>(input: &'a str) -> IResult<&'a str, Self, E>
    where
        E: ParseError<&'a str> + ContextError<&'a str>,
    {
        let (input, _) = context("password", char('x')).parse(input)?;
        Ok((input, Self::X))
    }
}

impl Display for GroupRecordPassword {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        match self {
            Self::X => "x".fmt(f),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupRecord<'a> {
    pub name: Cow<'a, str>,
    pub password: GroupRecordPassword,
    pub gid: GroupId,
    pub users: Vec<Cow<'a, str>>,
}

impl<'a> GroupRecord<'a> {
    fn parse<E>(input: &'a str) -> IResult<&'a str, Self, E>
    where
        E: ParseError<&'a str> + ContextError<&'a str>,
    {
        let (input, (name, _, password, _, gid, _)) = (
            context("groupname", take_until1(":")),
            char(':'),
            GroupRecordPassword::parse,
            char(':'),
            context("gid", nom::character::complete::u32),
            char(':'),
        )
            .parse(input)?;
        let (input, users) = take_until("\n").parse(input)?;
        let (_, users) = context(
            "users",
            // all_consuming(separated_list0(char(','), alphanumeric1)),
            all_consuming(separated_list0(char(','), recognize(many1(none_of(","))))),
        )
        .parse(users)?;
        Ok((
            input,
            Self {
                name: Cow::Borrowed(name),
                password,
                gid: gid.into(),
                users: users.into_iter().map(Cow::Borrowed).collect(),
            },
        ))
    }

    pub fn into_owned(self) -> GroupRecord<'static> {
        GroupRecord {
            name: Cow::Owned(self.name.into_owned()),
            password: self.password,
            gid: self.gid,
            users: self
                .users
                .into_iter()
                .map(Cow::into_owned)
                .map(Cow::Owned)
                .collect(),
        }
    }
}

impl<'a> Display for GroupRecord<'a> {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        write!(f, "{}:{}:{}:", self.name, self.password, self.gid.as_raw())?;
        for (i, u) in self.users.iter().enumerate() {
            write!(f, "{u}")?;
            if i < self.users.len() - 1 {
                write!(f, ",")?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn parse_etc_group() {
        let src = r#"root:x:0:
bin:x:1:root,daemon
daemon:x:2:root,bin
sys:x:3:root,bin,adm
adm:x:4:
systemd-journal:x:190:systemd-journald
"#;
        let groups = EtcGroup::parse(src).expect("failed to parse");
        // easy way to check that all the contents were parsed
        assert_eq!(src, groups.to_string());
        assert_eq!(
            Some(&GroupRecord {
                name: "bin".into(),
                password: GroupRecordPassword::X,
                gid: 1.into(),
                users: vec!["root".into(), "daemon".into()],
            }),
            groups.get_group_by_id(1.into()),
        );
    }

    #[test]
    fn parse_with_blank_trailing_lines() {
        let src = "root:x:0:\n\n";
        let groups = EtcGroup::parse(src).expect("failed to parse");
        assert_eq!(
            Some(&GroupRecord {
                name: "root".into(),
                password: GroupRecordPassword::X,
                gid: 0.into(),
                users: Vec::new()
            }),
            groups.get_group_by_id(0.into())
        );
    }
}
