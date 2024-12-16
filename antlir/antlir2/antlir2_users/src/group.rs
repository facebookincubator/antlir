/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Parse `/etc/group` files to get a map of all the groups inside of an
//! under-construction image so that ownership is attributed properly.

use std::borrow::Cow;
use std::collections::btree_map;
use std::collections::btree_map::BTreeMap;
use std::collections::BTreeSet;
use std::fmt::Display;
use std::fmt::Formatter;
use std::str::FromStr;

use maplit::btreemap;
use nom::bytes::complete::take_until;
use nom::bytes::complete::take_until1;
use nom::character::complete::char;
use nom::character::complete::newline;
use nom::character::complete::none_of;
use nom::combinator::all_consuming;
use nom::combinator::recognize;
use nom::error::context;
use nom::error::convert_error;
use nom::error::ContextError;
use nom::error::ParseError;
use nom::error::VerboseError;
use nom::multi::many0;
use nom::multi::many1;
use nom::multi::separated_list0;
use nom::sequence::tuple;
use nom::Finish;
use nom::IResult;

use crate::Error;
use crate::GroupId;
use crate::Id;
use crate::Result;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EtcGroup<'a> {
    // BTreeMap is used to prevent duplicate entries
    // with the same groupname.
    records: BTreeMap<String, GroupRecord<'a>>,
}

impl<'a> Default for EtcGroup<'a> {
    fn default() -> Self {
        Self {
            records: btreemap! {
                "root".into() => GroupRecord {
                    name: "root".into(),
                    gid: GroupId(0),
                    users: vec!["root".into()],
                },
            },
        }
    }
}

impl<'a> EtcGroup<'a> {
    fn parse_internal<E>(input: &'a str) -> IResult<&'a str, Self, E>
    where
        E: ParseError<&'a str> + ContextError<&'a str>,
    {
        let (input, records) =
            separated_list0(newline, context("GroupRecord", GroupRecord::parse))(input)?;
        // eat trailing newlines
        let (input, _) = all_consuming(many0(newline))(input)?;
        Ok((
            input,
            Self {
                records: records
                    .into_iter()
                    .map(|r| (r.name.to_string(), r))
                    .collect(),
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
        self.records.values()
    }

    pub fn into_records(self) -> impl Iterator<Item = GroupRecord<'a>> {
        self.records.into_values()
    }

    pub fn push(&mut self, record: GroupRecord<'a>) -> Result<()> {
        match self.records.entry(record.name.to_string()) {
            btree_map::Entry::Vacant(e) => {
                e.insert(record);
                Ok(())
            }
            btree_map::Entry::Occupied(e) if e.get() == &record => Ok(()),
            btree_map::Entry::Occupied(e) => Err(Error::Duplicate(
                e.get().name.to_string(),
                format!("{:?}", e.get()),
                format!("{:?}", record),
            )),
        }
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    pub fn get_group_by_name(&self, name: &str) -> Option<&GroupRecord<'a>> {
        self.records.get(name)
    }

    pub fn get_group_by_name_mut(&mut self, name: &str) -> Option<&mut GroupRecord<'a>> {
        self.records.get_mut(name)
    }

    pub fn get_group_by_id(&self, id: GroupId) -> Option<&GroupRecord<'a>> {
        self.records.values().find(|r| r.gid == id)
    }

    pub fn into_owned(self) -> EtcGroup<'static> {
        EtcGroup {
            records: self
                .records
                .into_iter()
                .map(|(name, record)| (name, record.into_owned()))
                .collect(),
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

// When printing the file, we want to use Ord implementation of GroupRecord.
// This way, the file will resemble a file created the regular way (adduser/addgroup).
impl<'a> Display for EtcGroup<'a> {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        let records = self.records.values().collect::<BTreeSet<_>>();
        for record in records {
            writeln!(f, "{}", record)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct GroupRecord<'a> {
    // Keep as the first field so we sort by it.
    pub gid: GroupId,
    pub name: Cow<'a, str>,
    pub users: Vec<Cow<'a, str>>,
}

impl<'a> GroupRecord<'a> {
    fn parse<E>(input: &'a str) -> IResult<&'a str, Self, E>
    where
        E: ParseError<&'a str> + ContextError<&'a str>,
    {
        let colon = char(':');
        let (input, (name, _, _, _, gid, _)) = tuple((
            context("groupname", take_until1(":")),
            &colon,
            // On modern Unix systems, password field is always "x".
            char('x'),
            &colon,
            context("gid", nom::character::complete::u32),
            &colon,
        ))(input)?;
        let (input, users) = take_until("\n")(input)?;
        let (_, users) = context(
            "users",
            // all_consuming(separated_list0(char(','), alphanumeric1)),
            all_consuming(separated_list0(char(','), recognize(many1(none_of(","))))),
        )(users)?;
        Ok((
            input,
            Self {
                name: Cow::Borrowed(name),
                gid: gid.into(),
                users: users.into_iter().map(Cow::Borrowed).collect(),
            },
        ))
    }

    pub fn into_owned(self) -> GroupRecord<'static> {
        GroupRecord {
            name: Cow::Owned(self.name.into_owned()),
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
        write!(f, "{}:x:{}:", self.name, self.gid.as_raw())?;
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
                gid: 0.into(),
                users: Vec::new()
            }),
            groups.get_group_by_id(0.into())
        );
    }
}
