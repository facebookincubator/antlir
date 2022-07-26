// (c) Facebook, Inc. and its affiliates. Confidential and proprietary.

use slog::error;
use slog::info;
use slog::warn;
use std::fs::File;
use std::fs::{self};
use std::io::BufRead;
use std::io::BufReader;
use std::io::Read;
use std::path::Path;
use std::path::PathBuf;

use identity::Identity;
use identity::IdentitySet;
use permission_checker::PermissionsChecker;
use permission_checker::{self};

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Failed to parse identity {0:?}")]
    ParseIdentity(anyhow::Error),
    #[error("Failed to load fallback identity file(s): {0:?}")]
    LoadIdentityFile(std::io::Error),
}

pub type Result<R> = std::result::Result<R, Error>;

#[derive(Clone)]
pub struct FallbackIdentityChecker<C: ?Sized>
where
    C: PermissionsChecker,
{
    checker: Box<C>,
    identities_path: Vec<PathBuf>,
}

impl<C> FallbackIdentityChecker<C>
where
    C: PermissionsChecker,
{
    pub fn new(checker: C, identities_path: Vec<PathBuf>) -> Self {
        info!(logging::get(), "FallbackIdentityChecker initiated.");
        Self {
            checker: Box::new(checker),
            identities_path,
        }
    }

    pub fn get_allowed_identities_from_files(&self) -> Result<Vec<Identity>> {
        let mut identities: Vec<Identity> = Vec::new();
        for dir in &self.identities_path {
            for filename in get_list_of_files(dir)? {
                let file = File::open(filename).map_err(Error::LoadIdentityFile)?;
                let reader = IdentityFileReader::new(file);
                identities.append(&mut reader.identities()?);
            }
        }
        Ok(identities)
    }
}

fn get_list_of_files(path: impl AsRef<Path>) -> Result<Vec<PathBuf>> {
    match fs::read_dir(path).map_err(Error::LoadIdentityFile) {
        Ok(paths) => paths
            .map(|entry| Ok(entry.map_err(Error::LoadIdentityFile)?.path()))
            .collect(),
        Err(error) => {
            error!(logging::get(), "{:?}", error);
            Ok(vec![])
        }
    }
}

// Allow FallbackIdentityChecker to be used as PermissionsChecker in Metald.
impl<C> PermissionsChecker for FallbackIdentityChecker<C>
where
    C: PermissionsChecker,
{
    fn action_allowed_for_identity(
        &self,
        ids: &IdentitySet,
        acl_action: &str,
    ) -> permission_checker::Result<bool> {
        match self.checker.action_allowed_for_identity(ids, acl_action) {
            Ok(res) => Ok(res),
            Err(error) => {
                warn!(
                    logging::get(),
                    "Internal checker failed with error: {:?}, checking fallback list", error
                );
                let fallback_identities = self
                    .get_allowed_identities_from_files()
                    .map_err(|_| permission_checker::Error::GetFallbackIdentityError)?;
                for accessor in ids.entries().iter() {
                    for id in fallback_identities.iter() {
                        if accessor.get_type() == id.get_type()
                            && accessor.get_data() == id.get_data()
                        {
                            info!(logging::get(), "Identity allowed as per fallback list.");
                            return Ok(true);
                        }
                    }
                }
                let error = Err(permission_checker::Error::CheckFallbackIdentityError {
                    identities: ids.to_string(),
                });
                error!(logging::get(), "{:?}", error);
                error
            }
        }
    }
}

pub struct IdentityFileReader<R: Read> {
    reader: BufReader<R>,
}

impl<R: Read> IdentityFileReader<R> {
    pub fn new(reader: R) -> Self {
        Self {
            reader: BufReader::new(reader),
        }
    }

    pub fn identities(self) -> Result<Vec<Identity>> {
        // parse identities from reader
        let mut read_lines: Vec<String> = Vec::new();
        for identity in self.reader.lines() {
            // convert identities from string to Identity
            let identity = identity.map_err(Error::LoadIdentityFile)?;
            read_lines.push(identity);
        }
        read_lines
            .into_iter()
            .map(|line| line.parse().map_err(Error::ParseIdentity))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockall::mock;
    use std::any::Any;
    use std::any::TypeId;
    use std::io::Write;
    use tempfile::tempdir;
    use tempfile::tempfile;

    mock! {
        MyPermissionsChecker {}
        impl PermissionsChecker for MyPermissionsChecker {
            fn action_allowed_for_identity(
                &self,
                ids: &IdentitySet,
                acl_action: &str,
            ) -> permission_checker::Result<bool>;
        }
    }

    #[fbinit::test]
    fn test_init_identity_file_reader() {
        let filename = tempfile().unwrap();
        let identity_reader = IdentityFileReader::new(filename);
        assert_eq!(
            TypeId::of::<IdentityFileReader<File>>(),
            identity_reader.type_id()
        );
        assert_eq!(
            TypeId::of::<BufReader<File>>(),
            identity_reader.reader.type_id()
        );
    }

    #[fbinit::test]
    fn test_identities_pass() {
        let mut filename = tempfile().unwrap();
        writeln!(filename, "USER:myuser\nUSER:anotheruser").unwrap();
        let identity_reader = IdentityFileReader::new(filename);

        let identities = identity_reader.identities().unwrap();
        let mut expected = vec![
            Identity::new("USER", "myuser"),
            Identity::new("USER", "anotheruser"),
        ];
        for couple in identities.iter().zip(expected.iter_mut()) {
            let (out_id, expect_id) = couple;
            assert_eq!(out_id.get_type(), expect_id.get_type());
            assert_eq!(out_id.get_data(), expect_id.get_data())
        }
    }

    #[fbinit::test]
    fn test_init_fallback_identity_checker() {
        let permission_checker = MockMyPermissionsChecker::new();
        let fallback_identity_checker =
            FallbackIdentityChecker::new(permission_checker, vec![PathBuf::from("test/path")]);
        assert_eq!(
            TypeId::of::<FallbackIdentityChecker<MockMyPermissionsChecker>>(),
            fallback_identity_checker.type_id()
        );
        assert_eq!(
            TypeId::of::<Box<MockMyPermissionsChecker>>(),
            fallback_identity_checker.checker.type_id()
        );
    }

    #[fbinit::test]
    fn test_get_list_of_files_pass() {
        let files = vec![
            "allowed_entities".to_string(),
            "more_allowed_entities".to_string(),
        ];
        let tmp_dir = tempdir().unwrap();
        let mut expected: Vec<PathBuf> = vec![];
        for file in files {
            let file_path = tmp_dir.path().join(file);
            File::create(file_path.clone()).unwrap();
            expected.push(file_path)
        }
        let output = get_list_of_files(&tmp_dir.into_path()).unwrap();
        assert_eq!(output, expected);
    }

    #[fbinit::test]
    fn test_get_list_of_files_path_dir_not_found() {
        match get_list_of_files(&PathBuf::from("not/existent/path")) {
            Ok(output) => {
                let expected: Vec<PathBuf> = vec![];
                assert_eq!(output, expected)
            }
            Err(error) => {
                let expected = "Failed to load fallback identity file(s): Os { code: 2, kind: NotFound, message: \"No such file or directory\" }";
                assert_eq!(format!("{}", error), expected)
            }
        };
    }

    #[fbinit::test]
    fn test_get_allowed_identities_from_files_pass() {
        let permission_checker = MockMyPermissionsChecker::new();

        let tmp_dir = tempdir().unwrap();
        let file_path = tmp_dir.path().join("allowed_entities");
        let mut file1 = File::create(file_path).unwrap();
        writeln!(file1, "USER:myuser\nUSER:anotheruser").unwrap();
        let file_path = tmp_dir.path().join("more_allowed_entities");
        let mut file2 = File::create(file_path).unwrap();
        writeln!(file2, "USER:moreuser").unwrap();

        let fallback_identity_checker =
            FallbackIdentityChecker::new(permission_checker, vec![tmp_dir.into_path()]);
        let identities = fallback_identity_checker
            .get_allowed_identities_from_files()
            .unwrap();
        let mut expected = vec![
            Identity::new("USER", "myuser"),
            Identity::new("USER", "anotheruser"),
            Identity::new("USER", "moreuser"),
        ];
        for couple in identities.iter().zip(expected.iter_mut()) {
            let (out_id, expect_id) = couple;
            assert_eq!(out_id.get_type(), expect_id.get_type());
            assert_eq!(out_id.get_data(), expect_id.get_data())
        }
    }

    #[fbinit::test]
    fn test_get_allowed_identities_from_files_path_dir_not_found() {
        let permission_checker = MockMyPermissionsChecker::new();
        let fallback_identity_checker = FallbackIdentityChecker::new(
            permission_checker,
            vec![PathBuf::from("not/existent/path")],
        );
        match fallback_identity_checker.get_allowed_identities_from_files() {
            Ok(identities) => {
                let expected: Vec<Identity> = vec![];
                assert_eq!(identities, expected)
            }
            Err(error) => {
                let expected = "Failed to load fallback identity file(s): Os { code: 2, kind: NotFound, message: \"No such file or directory\" }";
                assert_eq!(format!("{}", error), expected)
            }
        };
    }

    #[fbinit::test]
    fn test_action_allowed_for_identity_pass() {
        // init fb_acl_checker and expected returns
        let tmp_dir = tempdir().unwrap();
        let mut permission_checker = MockMyPermissionsChecker::new();
        permission_checker
            .expect_action_allowed_for_identity()
            .returning(|_, _| Ok(true));
        let fallback_identity_checker =
            FallbackIdentityChecker::new(permission_checker, vec![tmp_dir.into_path()]);

        let ids: IdentitySet = vec![
            Identity::new("USER", "myuser"),
            Identity::new("USER", "anotheruser"),
        ]
        .into_iter()
        .collect();

        // test
        let result = fallback_identity_checker.action_allowed_for_identity(&ids, "test_action");
        assert_eq!(true, result.unwrap())
    }

    #[fbinit::test]
    fn test_action_allowed_for_identity_with_fallback_pass() {
        // init fb_acl_checker and expected returns
        let tmp_dir = tempdir().unwrap();
        let file_path = tmp_dir.path().join("allowed_entities");
        let mut file1 = File::create(file_path).unwrap();
        writeln!(file1, "USER:myuserother\nUSER:alloweduser").unwrap();

        let mut permission_checker = MockMyPermissionsChecker::new();
        permission_checker
            .expect_action_allowed_for_identity()
            .returning(|_, _| {
                Err(permission_checker::Error::CheckIdentityError {
                    acl_name: "test_acl".to_string(),
                    action: "test_action".to_string(),
                    identities: "test_ids".to_string(),
                })
            });
        let fallback_identity_checker =
            FallbackIdentityChecker::new(permission_checker, vec![tmp_dir.into_path()]);

        let ids: IdentitySet = vec![
            Identity::new("USER", "myuser"),
            Identity::new("USER", "alloweduser"),
            Identity::new("USER", "anotheruser"),
        ]
        .into_iter()
        .collect();

        // test
        let result = fallback_identity_checker.action_allowed_for_identity(&ids, "test_action");
        assert_eq!(true, result.unwrap())
    }

    #[fbinit::test]
    fn test_action_allowed_for_identity_with_fallback_fail() {
        // init allowed identities
        let tmp_dir = tempdir().unwrap();
        let file_path = tmp_dir.path().join("allowed_entities");
        let mut file1 = File::create(file_path).unwrap();
        writeln!(file1, "USER:myuserother\nUSER:alloweduser").unwrap();

        // set the checker to fail
        let mut permission_checker = MockMyPermissionsChecker::new();
        permission_checker
            .expect_action_allowed_for_identity()
            .returning(|_, _| {
                Err(permission_checker::Error::CheckIdentityError {
                    acl_name: "test_acl".to_string(),
                    action: "test_action".to_string(),
                    identities: "test_ids".to_string(),
                })
            });
        let fallback_identity_checker =
            FallbackIdentityChecker::new(permission_checker, vec![tmp_dir.into_path()]);

        let ids: IdentitySet = vec![
            Identity::new("USER", "myuser"),
            Identity::new("USER", "notalloweduser"),
            Identity::new("USER", "anotheruser"),
        ]
        .into_iter()
        .collect();

        // test
        let result = fallback_identity_checker.action_allowed_for_identity(&ids, "test_action");
        match result {
            Ok(_) => panic!("No error found {:?}", result),
            Err(error) => {
                let expected = "Requester USER:anotheruser,USER:notalloweduser,USER:myuser not in the authorized fallback list.";
                assert_eq!(format!("{}", error), expected)
            }
        };
    }
}
