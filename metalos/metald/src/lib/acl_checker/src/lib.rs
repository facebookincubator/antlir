// (c) Facebook, Inc. and its affiliates. Confidential and proprietary.

use std::time::Duration;

use aclchecker::AclChecker;
use identity::IdentitySet;
use mockall::*;
use permission_checker::PermissionsChecker;
use slog::error;
use slog::info;

// For testing purpose we need an AclChecker Wrapper trait that can be mocked.
// Wrapping aclchecker::AclChecker
#[automock]
pub trait FbAclCheckerWrapper: Send + Sync {
    fn do_wait_updated(&self, duration_ms: Duration) -> bool;
    fn check_set<'a>(&self, accessors: &IdentitySet, actions: &[&'a str]) -> bool;
}

impl FbAclCheckerWrapper for AclChecker {
    fn do_wait_updated(&self, duration_ms: Duration) -> bool {
        self.do_wait_updated(duration_ms.as_millis() as u32)
    }
    fn check_set<'a>(&self, accessors: &IdentitySet, actions: &[&'a str]) -> bool {
        self.check_set(accessors, actions)
    }
}

#[derive(Clone)]
pub struct AclCheckerService<C>
where
    C: FbAclCheckerWrapper,
{
    pub fb_acl_checker: Box<C>,
    pub acl_name: String,
}

impl<C> AclCheckerService<C>
where
    C: FbAclCheckerWrapper,
{
    pub fn new(fb_acl_checker: C, acl_name: &str) -> Self {
        let timeout = Duration::from_millis(10_000);
        if !fb_acl_checker.do_wait_updated(timeout) {
            // report the error, but don't break.
            error!(
                logging::get(),
                "Timed out after {0:?} while waiting for ACL checker config to load",
                timeout.as_secs()
            );
        };
        info!(logging::get(), "AclChecker initiated.");
        Self {
            fb_acl_checker: Box::new(fb_acl_checker),
            acl_name: acl_name.to_string(),
        }
    }
}

// Allow AclChecker to be used as PermissionsChecker in Metald.
impl<C> PermissionsChecker for AclCheckerService<C>
where
    C: FbAclCheckerWrapper,
{
    fn action_allowed_for_identity(
        &self,
        ids: &IdentitySet,
        acl_action: &str,
    ) -> permission_checker::Result<bool> {
        if self.fb_acl_checker.check_set(ids, &[acl_action]) {
            return Ok(true);
        };
        Err(permission_checker::Error::CheckIdentityError {
            acl_name: self.acl_name.clone(),
            action: acl_action.to_string(),
            identities: format!("{:?}", ids),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::any::Any;
    use std::any::TypeId;

    use srserver::RequestContext;

    use super::*;

    #[fbinit::test]
    fn test_init_aclchecker_pass() {
        // init fb_acl_checker and expected returns
        let acl_name: &str = "test_acl";
        let mut fb_acl_checker = MockFbAclCheckerWrapper::new();
        fb_acl_checker.expect_do_wait_updated().return_const(true);

        // init acl_checker object
        let acl_checker = AclCheckerService::new(fb_acl_checker, acl_name);

        // test
        assert_eq!(
            TypeId::of::<AclCheckerService<MockFbAclCheckerWrapper>>(),
            acl_checker.type_id()
        );
        assert_eq!(
            TypeId::of::<Box<MockFbAclCheckerWrapper>>(),
            acl_checker.fb_acl_checker.type_id()
        );
    }

    #[fbinit::test]
    fn test_init_aclchecker_fail_and_recover() {
        // init fb_acl_checker and expected returns
        let acl_name: &str = "test_acl";
        let mut fb_acl_checker = MockFbAclCheckerWrapper::new();
        fb_acl_checker.expect_do_wait_updated().return_const(false);

        // init acl_checker object
        let acl_checker = AclCheckerService::new(fb_acl_checker, acl_name);

        // test
        assert_eq!(
            TypeId::of::<AclCheckerService<MockFbAclCheckerWrapper>>(),
            acl_checker.type_id()
        );
        assert_eq!(
            TypeId::of::<Box<MockFbAclCheckerWrapper>>(),
            acl_checker.fb_acl_checker.type_id()
        );
    }

    #[fbinit::test]
    fn test_action_allowed_for_identity_pass() {
        // init fb_acl_checker and expected returns
        let acl_name: &str = "test_acl";
        let mut fb_acl_checker = MockFbAclCheckerWrapper::new();
        fb_acl_checker.expect_do_wait_updated().return_const(true);
        fb_acl_checker.expect_check_set().return_const(true);

        // init acl_checker object
        let acl_checker = AclCheckerService::new(fb_acl_checker, acl_name);
        let req_ctxt = RequestContext::new_test_stub();
        let ids = req_ctxt.identities().unwrap();

        // test
        let result = acl_checker.action_allowed_for_identity(&ids, "test_action");
        assert_eq!(true, result.unwrap())
    }

    #[fbinit::test]
    fn test_action_allowed_for_identity_fail() {
        // init fb_acl_checker and expected returns
        let acl_name: &str = "test_acl";
        let mut fb_acl_checker = MockFbAclCheckerWrapper::new();
        fb_acl_checker.expect_do_wait_updated().return_const(true);
        fb_acl_checker.expect_check_set().return_const(false);

        // init acl_checker object
        let acl_checker = AclCheckerService::new(fb_acl_checker, acl_name);
        let req_ctxt = RequestContext::new_test_stub();
        let ids = req_ctxt.identities().unwrap();

        // test
        let result = acl_checker.action_allowed_for_identity(&ids, "test_action");
        match result {
            Ok(_) => panic!("No error found {:?}", result),
            Err(error) => {
                let expected = "Requester IdentitySet did not pass action test_action on test_acl";
                assert_eq!(format!("{}", error), expected)
            }
        };
    }
}
