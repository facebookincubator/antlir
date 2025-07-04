/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Parse `/etc/passwd` files to get a map of all the users inside of an
//! under-construction image so that ownership is attributed properly.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fmt::Display;
use std::fmt::Formatter;
use std::path::Path;
use std::str::FromStr;

use maplit::btreemap;
use nom::Finish;
use nom::IResult;
use nom::Parser as _;
use nom::bytes::complete::take_until;
use nom::bytes::complete::take_until1;
use nom::character::complete::char;
use nom::character::complete::newline;
use nom::combinator::all_consuming;
use nom::error::ContextError;
use nom::error::ParseError;
use nom::error::context;
use nom::multi::many0;
use nom::multi::separated_list0;
use nom_language::error::VerboseError;
use nom_language::error::convert_error;

use crate::Error;
use crate::GroupId;
use crate::Id;
use crate::Result;
use crate::UserId;
use crate::shadow::ShadowRecord;
use crate::shadow::ShadowRecordPassword;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EtcPasswd<'a> {
    records: Vec<UserRecord<'a>>,
    uid_to_record_idx: BTreeMap<UserId, usize>,
    username_to_record_idx: BTreeMap<String, usize>,
}

impl<'a> Default for EtcPasswd<'a> {
    fn default() -> Self {
        Self {
            records: vec![UserRecord {
                name: "root".into(),
                password: UserRecordPassword::Shadow,
                uid: UserId(0),
                gid: GroupId(0),
                comment: "".into(),
                homedir: Path::new("/root").into(),
                shell: Path::new("/bin/bash").into(),
            }],
            uid_to_record_idx: btreemap! {UserId(0) => 0},
            username_to_record_idx: btreemap! {"root".to_string() => 0},
        }
    }
}

impl<'a> EtcPasswd<'a> {
    fn parse_internal<E>(input: &'a str) -> IResult<&'a str, Self, E>
    where
        E: ParseError<&'a str> + ContextError<&'a str>,
    {
        let (input, records) =
            separated_list0(newline, context("UserRecord", UserRecord::parse)).parse(input)?;
        // eat any trailing newlines
        let (input, _) = all_consuming(many0(newline)).parse(input)?;
        Ok((
            input,
            Self {
                uid_to_record_idx: records
                    .iter()
                    .enumerate()
                    .map(|(idx, r)| (r.uid, idx))
                    .collect(),
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

    pub fn records(&self) -> impl Iterator<Item = &UserRecord<'a>> {
        self.records.iter()
    }

    pub fn into_records(self) -> impl Iterator<Item = UserRecord<'a>> {
        self.records.into_iter()
    }

    pub fn push(&mut self, record: UserRecord<'a>) -> Result<()> {
        match (
            self.get_user_by_id(record.uid),
            self.get_user_by_name(&record.name),
        ) {
            (Some(existing), _) | (_, Some(existing)) if *existing == record => Ok(()),
            (Some(existing), _) | (_, Some(existing)) => Err(Error::Duplicate(
                existing.name.to_string(),
                format!("{:?}", existing),
                format!("{:?}", record),
            )),
            (None, None) => {
                self.uid_to_record_idx
                    .insert(record.uid, self.records.len());
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

    pub fn get_user_by_name(&self, name: &str) -> Option<&UserRecord<'a>> {
        self.username_to_record_idx
            .get(name)
            .and_then(|&idx| self.records.get(idx))
    }

    pub fn get_user_by_id(&self, id: UserId) -> Option<&UserRecord<'a>> {
        self.uid_to_record_idx
            .get(&id)
            .and_then(|&idx| self.records.get(idx))
    }

    pub fn into_owned(self) -> EtcPasswd<'static> {
        EtcPasswd {
            records: self
                .records
                .into_iter()
                .map(UserRecord::into_owned)
                .collect(),
            uid_to_record_idx: self.uid_to_record_idx,
            username_to_record_idx: self.username_to_record_idx,
        }
    }
}

impl FromStr for EtcPasswd<'static> {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        let s = EtcPasswd::parse(s)?;
        Ok(s.into_owned())
    }
}

impl<'a> Display for EtcPasswd<'a> {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        for record in &self.records {
            writeln!(f, "{}", record)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UserRecordPassword {
    /// Store authentication details in /etc/shadow instead.
    /// On modern systems, this is strongly recommended.
    Shadow,
    /// Lock the account, preventing login. Prefer using Shadow
    /// and locking the user in /etc/shadow instead.
    Locked,
    /// Empty string, login is allowed without a password at all.
    /// Prefer using Shadow and setting no password there.
    Empty,
}

impl UserRecordPassword {
    fn parse<'a, E>(input: &'a str) -> IResult<&'a str, Self, E>
    where
        E: ParseError<&'a str> + ContextError<&'a str>,
    {
        let (input, txt) = context(
            "password",
            nom::branch::alt((
                nom::bytes::complete::tag("x"),
                nom::bytes::complete::tag("!"),
                nom::bytes::complete::tag(""),
            )),
        )
        .parse(input)?;
        Ok((
            input,
            match txt {
                "x" => Self::Shadow,
                "!" => Self::Locked,
                "" => Self::Empty,
                _ => unreachable!("parser would have failed"),
            },
        ))
    }
}

impl Display for UserRecordPassword {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        match self {
            Self::Shadow => write!(f, "x"),
            Self::Locked => write!(f, "!"),
            Self::Empty => write!(f, ""),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserRecord<'a> {
    pub name: Cow<'a, str>,
    pub password: UserRecordPassword,
    pub uid: UserId,
    pub gid: GroupId,
    pub comment: Cow<'a, str>,
    pub homedir: Cow<'a, Path>,
    pub shell: Cow<'a, Path>,
}

impl<'a> UserRecord<'a> {
    fn parse<E>(input: &'a str) -> IResult<&'a str, Self, E>
    where
        E: ParseError<&'a str> + ContextError<&'a str>,
    {
        let (input, (name, _, password, _, uid, _, gid, _, comment, _, homedir, _, shell)) = (
            context("username", take_until1(":")),
            char(':'),
            UserRecordPassword::parse,
            char(':'),
            context("uid", nom::character::complete::u32),
            char(':'),
            context("gid", nom::character::complete::u32),
            char(':'),
            context("comment", take_until(":")),
            char(':'),
            context("homedir", take_until1(":")),
            char(':'),
            context("shell", take_until1("\n")),
        )
            .parse(input)?;
        Ok((
            input,
            Self {
                name: Cow::Borrowed(name),
                password,
                uid: uid.into(),
                gid: gid.into(),
                comment: Cow::Borrowed(comment),
                homedir: Cow::Borrowed(Path::new(homedir)),
                shell: Cow::Borrowed(Path::new(shell)),
            },
        ))
    }

    pub fn into_owned(self) -> UserRecord<'static> {
        UserRecord {
            name: Cow::Owned(self.name.into_owned()),
            password: self.password.clone(),
            uid: self.uid,
            gid: self.gid,
            comment: Cow::Owned(self.comment.into_owned()),
            homedir: Cow::Owned(self.homedir.into_owned()),
            shell: Cow::Owned(self.shell.into_owned()),
        }
    }

    /// Create a new, default shadow record for this user.
    pub fn new_shadow_record(&self) -> ShadowRecord<'a> {
        ShadowRecord {
            name: self.name.clone(),
            encrypted_password: match self.password {
                UserRecordPassword::Shadow | UserRecordPassword::Locked => {
                    ShadowRecordPassword::NoLogin
                }
                UserRecordPassword::Empty => ShadowRecordPassword::OpenLogin,
            },
            last_password_change: None,
            minimum_password_age: None,
            maximum_password_age: None,
            password_warning_period: None,
            password_inactivity_period: None,
            account_expiration_date: None,
            reserved: Cow::Borrowed(""),
        }
    }
}

impl<'a> Display for UserRecord<'a> {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        write!(
            f,
            "{}:{}:{}:{}:{}:{}:{}",
            self.name,
            self.password,
            self.uid.as_raw(),
            self.gid.as_raw(),
            self.comment,
            self.homedir.display(),
            self.shell.display()
        )
    }
}

#[cfg(test)]
mod tests {
    use nom_language::error::VerboseError;
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case::shadow("x", UserRecordPassword::Shadow)]
    #[case::shadow("!", UserRecordPassword::Locked)]
    #[case::shadow("", UserRecordPassword::Empty)]
    fn test_parse_password(#[case] input: &str, #[case] expected: UserRecordPassword) {
        let (rest, pw) =
            UserRecordPassword::parse::<VerboseError<&str>>(input).expect("failed to parse");
        assert_eq!(pw, expected);
        assert_eq!(rest, "", "all input should have been consumed");
    }

    #[test]
    fn parse_etc_passwd() {
        let src = r#"root:x:0:0:root:/root:/bin/bash
bin:x:1:1:bin:/bin:/sbin/nologin
daemon:x:2:2:daemon:/sbin:/sbin/nologin
adm:x:3:4:adm:/var/adm:/sbin/nologin
lp:x:4:7:lp:/var/spool/lpd:/sbin/nologin
sync:x:5:0:sync:/sbin:/bin/sync
shutdown:x:6:0:shutdown:/sbin:/sbin/shutdown
halt:x:7:0:halt:/sbin:/sbin/halt
mail:x:8:12:mail:/var/spool/mail:/sbin/nologin
operator:x:11:0:operator:/root:/sbin/nologin
games:x:12:100:games:/usr/games:/sbin/nologin
ftp:x:14:50:FTP User:/var/ftp:/sbin/nologin
tss:x:59:59:Account used for TPM access:/dev/null:/sbin/nologin
dbus:x:81:81:System message bus:/:/sbin/nologin
pwdlesslogin::420:420:Passwordless login:/dev/null:/sbin/nologin
systemd-oom:x:999:999:systemd Userspace OOM Killer:/:/usr/sbin/nologin
nobody:x:65534:65534:Kernel Overflow User:/:/sbin/nologin
"#;
        let passwd = EtcPasswd::parse(src).expect("failed to parse");
        // easy way to check that all the contents were parsed
        assert_eq!(src, passwd.to_string());
        assert_eq!(
            Some(&UserRecord {
                name: "root".into(),
                password: UserRecordPassword::Shadow,
                uid: 0.into(),
                gid: 0.into(),
                comment: "root".into(),
                homedir: Path::new("/root").into(),
                shell: Path::new("/bin/bash").into(),
            }),
            passwd.get_user_by_id(0.into()),
        );
        assert_eq!(
            Some(&UserRecord {
                name: "root".into(),
                password: UserRecordPassword::Shadow,
                uid: 0.into(),
                gid: 0.into(),
                comment: "root".into(),
                homedir: Path::new("/root").into(),
                shell: Path::new("/bin/bash").into(),
            }),
            passwd.get_user_by_name("root"),
        );
    }

    #[test]
    fn parse_trailing_blanks() {
        let src = "root:x:0:0:root:/root:/bin/bash\n\n\n";
        let passwd = EtcPasswd::parse(src).expect("failed to parse");
        assert_eq!(
            Some(&UserRecord {
                name: "root".into(),
                password: UserRecordPassword::Shadow,
                uid: 0.into(),
                gid: 0.into(),
                comment: "root".into(),
                homedir: Path::new("/root").into(),
                shell: Path::new("/bin/bash").into(),
            }),
            passwd.get_user_by_id(0.into()),
        );
    }
}
