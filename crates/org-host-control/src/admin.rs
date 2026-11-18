//! Administrator link calls on `host.sock` (A2, ADR-0022). Both are admitted like every
//! `host.sock` client (hello, the registered instance, the host's effective UID) and the host's
//! signing check. The link secret travels only in the reply.
use org_proto::Call;
use serde::{Deserialize, Serialize};

use crate::OneTimeLinkUrl;

/// `admin.link.initial`: setup's one registration link, bound to the frozen setup operation.
/// The host issues it only if no registration link was ever issued and no administrator
/// exists. A replay answers [`AdminLinkResult::AlreadyIssued`] with no secret.
pub struct AdminLinkInitial;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdminLinkInitialParams {
    /// The setup record's `setup_operation` (UUIDv7).
    pub setup_operation: String,
}

impl Call for AdminLinkInitial {
    const METHOD: &'static str = "admin.link.initial";
    type Params = AdminLinkInitialParams;
    type Result = AdminLinkResult;
}

/// `admin.reset`: after local presence and confirmation at the host, a new registration link
/// that cancels the previous one, while no administrator exists. Resetting an existing
/// administrator's password answers `unavailable` until A3.
pub struct AdminReset;

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdminResetParams {}

impl Call for AdminReset {
    const METHOD: &'static str = "admin.reset";
    type Params = AdminResetParams;
    type Result = AdminLinkResult;
}

/// The outcome of a link call. Not `Clone`: `Issued` carries the secret.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case", deny_unknown_fields)]
pub enum AdminLinkResult {
    /// A new registration link. Show it once; it expires at `expires_at_ms`.
    Issued {
        url: OneTimeLinkUrl,
        expires_at_ms: i64,
    },
    /// A link was already issued; no secret is returned again.
    AlreadyIssued,
    /// An administrator exists; no registration link is issued.
    AdministratorPresent,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn link_results_round_trip_and_debug_hides_the_url() {
        let issued = AdminLinkResult::Issued {
            url: OneTimeLinkUrl::new("http://127.0.0.1:7420/register#abc".into()).unwrap(),
            expires_at_ms: 5,
        };
        let json = serde_json::to_string(&issued).unwrap();
        assert_eq!(
            json,
            r#"{"outcome":"issued","url":"http://127.0.0.1:7420/register#abc","expires_at_ms":5}"#
        );
        assert!(!format!("{issued:?}").contains("abc"));
        let replay: AdminLinkResult =
            serde_json::from_str(r#"{"outcome":"already_issued"}"#).unwrap();
        assert!(matches!(replay, AdminLinkResult::AlreadyIssued));
        assert!(serde_json::from_str::<AdminLinkInitialParams>(
            r#"{"setup_operation":"x","login":"a"}"#
        )
        .is_err());
        assert!(serde_json::from_str::<AdminResetParams>(r#"{"x":1}"#).is_err());
        assert_eq!(AdminLinkInitial::METHOD, "admin.link.initial");
        assert_eq!(AdminReset::METHOD, "admin.reset");
    }
}
