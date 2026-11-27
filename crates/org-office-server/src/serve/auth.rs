//! Connection authority types: the launch grant key and who a connection is bound as.

/// The launch authority: the grant key that verifies the grants persons and the owner act with
/// on host channels, retained and zeroed when `Shared` drops. Grants are the only person path,
/// and every launch carries a grant key in its handoff.
pub(crate) struct LaunchAuth {
    grant_key: org_proto::GrantKey,
}

impl From<org_proto::GrantKey> for LaunchAuth {
    fn from(grant_key: org_proto::GrantKey) -> Self {
        Self { grant_key }
    }
}

impl LaunchAuth {
    /// The grant key, for tests of the launch normalization.
    #[cfg(test)]
    pub(crate) fn grant_key(&self) -> &org_proto::GrantKey {
        &self.grant_key
    }

    /// Whether `grant` carries this launch's MAC.
    pub(crate) fn verifies(&self, grant: &org_proto::Grant) -> bool {
        crate::grant::verify(&self.grant_key, grant)
    }
}

/// Which authority verified an [`Binding::External`] connection, so per-call liveness and
/// revocation enforce 1:1 against the exact authority that granted the bind (`TASK-233`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum ExternalAuthority {
    /// Granted by [`org_office_sessions::RunBindingVerifier::verify_binding`].
    Verifier,
    /// Granted by [`super::external::ExternalAuthPort::verify`].
    ExternalAuth,
}

/// Who a connection is, as far as the socket knows.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) enum Binding {
    /// Not bound, or the last bind failed.
    Anonymous,
    /// Bound as a member: `id` is the member's canonical id. `via_grant` when a person or the
    /// owner acts through a verified grant on a host channel, for that one call; an agent binds on
    /// the office socket without one.
    Member { id: String, via_grant: bool },
    /// Bound as an agent through an external (BYOA) lease's fast token (`TASK-137`): `id` is the
    /// agent, `session_id` the lease, and `authority` the port that granted it. Never via a grant;
    /// limited to collaboration calls.
    External {
        id: String,
        session_id: String,
        authority: ExternalAuthority,
    },
}

impl Binding {
    /// Whether the call is made through a verified grant (a person or the owner): the only kind
    /// that may send organization commands.
    pub(super) fn via_grant(&self) -> bool {
        matches!(
            self,
            Binding::Member {
                via_grant: true,
                ..
            }
        )
    }

    /// Whether the connection is bound at all.
    pub(super) fn is_bound(&self) -> bool {
        *self != Binding::Anonymous
    }

    /// The member the connection acts as, if it is bound as one.
    pub(super) fn member(&self) -> Option<&str> {
        match self {
            Binding::Member { id, .. } | Binding::External { id, .. } => Some(id),
            Binding::Anonymous => None,
        }
    }

    /// The lease session of an externally bound connection.
    pub(super) fn external_session(&self) -> Option<&str> {
        match self {
            Binding::External { session_id, .. } => Some(session_id),
            _ => None,
        }
    }
}
