use identity::IdentitySet;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Requester {identities} did not pass action {action} on {acl_name}")]
    CheckIdentityError {
        acl_name: String,
        action: String,
        identities: String,
    },
}

pub type Result<R> = std::result::Result<R, Error>;

// Generic Permission Checker trait that needs to be implemented for
// Checker to be used by Metald
pub trait PermissionsChecker {
    fn action_allowed_for_identity(&self, ids: &IdentitySet, acl_action: &str) -> Result<bool>;
}
