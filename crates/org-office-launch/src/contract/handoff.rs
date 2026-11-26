//! The host's handoff on the rendezvous connection: its header line, the descriptors it names and
//! the secrets it borrows.

use super::*;

/// A descriptor named by a [`Handoff`] header, in the order they arrive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandoffFd {
    /// The server's end of its transfer socket.
    Transfer,
}

/// The host's answer to [`ReadyForSecrets`] on the rendezvous connection: one JSON line with the
/// secrets, naming the descriptors the following `sendmsg` carries. Its op matches the first
/// line's. Not `Clone`: it holds the keys. Encode it with [`encode_handoff`] and zero the bytes
/// after sending them.
///
/// It decodes as one flat object, not through serde's internally tagged enums, so each secret
/// goes straight from the JSON into its secret type without an intermediate buffer. A field the
/// op doesn't carry is refused, even as `null`.
#[derive(Debug, Serialize)]
#[serde(tag = "op", rename_all = "snake_case")]
#[non_exhaustive]
pub enum Handoff {
    /// After a create line (HQ): the new office's secrets key (required), the grant key and,
    /// when the create has one, the recovery code (absent otherwise, never `null`). `fds` is
    /// `["transfer"]`.
    Create {
        office_secrets_key: OfficeSecretsKey,
        grant_key: GrantKey,
        #[serde(skip_serializing_if = "Option::is_none")]
        recovery_code: Option<RecoveryCode>,
        fds: Vec<HandoffFd>,
    },
    /// After a join line (satellite): the office's secrets key, grant key, and one-time
    /// invitation secret (never in argv or first line). `fds` is `["transfer"]`.
    Join {
        office_secrets_key: OfficeSecretsKey,
        grant_key: GrantKey,
        invitation: InvitationSecret,
        fds: Vec<HandoffFd>,
    },
    /// After a start line: the office secrets key, or `null` when the host has none for that
    /// office (present either way), and the grant key. A start without a key proceeds: the
    /// office serves its data and only what the key unwraps is unavailable. `fds` is
    /// `["transfer"]`.
    Start {
        office_secrets_key: Option<OfficeSecretsKey>,
        grant_key: GrantKey,
        fds: Vec<HandoffFd>,
    },
    /// After a discard line: no secret at all (a plain office opens nothing that needs a key).
    /// `fds` is `[]`: no transfer socket, and no marker follows.
    Discard { fds: Vec<HandoffFd> },
}

/// A present key, `null` included, as `Some`: so a key the op doesn't carry is refused even as
/// `null`, and an absent one stays `None` through `#[serde(default)]`.
pub(super) fn present<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
    d: D,
) -> Result<Option<Option<T>>, D::Error> {
    Option::<T>::deserialize(d).map(Some)
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum HandoffOp {
    Create,
    Join,
    Start,
    Discard,
}

/// A handoff as read: every field at the top level, each secret decoded in place. A field it
/// doesn't define (a retired `app_token` or `db_key` included) is refused; `v` is checked before
/// the decode.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RawHandoff {
    #[serde(default, rename = "v")]
    _version: Option<IgnoredAny>,
    op: HandoffOp,
    #[serde(default, deserialize_with = "present")]
    office_secrets_key: Option<Option<OfficeSecretsKey>>,
    #[serde(default, deserialize_with = "present")]
    grant_key: Option<Option<GrantKey>>,
    #[serde(default, deserialize_with = "present")]
    recovery_code: Option<Option<RecoveryCode>>,
    #[serde(default, deserialize_with = "present")]
    invitation: Option<Option<InvitationSecret>>,
    fds: Vec<HandoffFd>,
}

impl<'de> Deserialize<'de> for Handoff {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        use serde::de::Error as _;
        let raw = RawHandoff::deserialize(d)?;
        let fds = raw.fds;
        match (
            raw.op,
            raw.office_secrets_key,
            raw.grant_key,
            raw.recovery_code,
            raw.invitation,
        ) {
            (
                HandoffOp::Create,
                Some(Some(office_secrets_key)),
                Some(Some(grant_key)),
                recovery_code @ (None | Some(Some(_))),
                None,
            ) => Ok(Handoff::Create {
                office_secrets_key,
                grant_key,
                recovery_code: recovery_code.flatten(),
                fds,
            }),
            (
                HandoffOp::Join,
                Some(Some(office_secrets_key)),
                Some(Some(grant_key)),
                None,
                Some(Some(invitation)),
            ) => Ok(Handoff::Join {
                office_secrets_key,
                grant_key,
                invitation,
                fds,
            }),
            (HandoffOp::Start, Some(office_secrets_key), Some(Some(grant_key)), None, None) => {
                Ok(Handoff::Start {
                    office_secrets_key,
                    grant_key,
                    fds,
                })
            }
            (HandoffOp::Discard, None, None, None, None) => Ok(Handoff::Discard { fds }),
            _ => Err(D::Error::custom(
                "a handoff must carry exactly the fields of its op",
            )),
        }
    }
}

impl Handoff {
    /// The descriptors this header must name, in order.
    pub fn expected_fds(&self) -> &'static [HandoffFd] {
        match self {
            Handoff::Create { .. } | Handoff::Join { .. } | Handoff::Start { .. } => {
                &[HandoffFd::Transfer]
            }
            Handoff::Discard { .. } => &[],
        }
    }

    /// The descriptors it names.
    pub fn fds(&self) -> &[HandoffFd] {
        match self {
            Handoff::Create { fds, .. }
            | Handoff::Join { fds, .. }
            | Handoff::Start { fds, .. }
            | Handoff::Discard { fds, .. } => fds,
        }
    }

    pub(super) fn check(&self) -> Result<(), LaunchError> {
        if self.fds() == self.expected_fds() {
            Ok(())
        } else {
            Err(LaunchError::Invalid(
                "the handoff names other descriptors than its op carries",
            ))
        }
    }
}

/// Encode a handoff as one line with its `v` and `\n`. A header naming the wrong descriptors is
/// refused. The bytes hold the keys: zero them after sending.
pub fn encode_handoff(handoff: &Handoff) -> Result<Vec<u8>, LaunchError> {
    handoff.check()?;
    encode_secret(handoff)
}

/// Borrow host-held secrets for a start without creating another owned key copy. The office
/// secrets key is `None` when the host has none for the office: it goes out as `null` and the
/// start proceeds. The returned line is secret and must be zeroed by the sender on every path.
pub fn encode_start_handoff(
    office_secrets_key: Option<&OfficeSecretsKey>,
    grant_key: &GrantKey,
) -> Result<Vec<u8>, LaunchError> {
    #[derive(Serialize)]
    struct Start<'a> {
        op: &'static str,
        office_secrets_key: Option<&'a OfficeSecretsKey>,
        grant_key: &'a GrantKey,
        fds: [HandoffFd; 1],
    }
    encode_secret(&Start {
        op: "start",
        office_secrets_key,
        grant_key,
        fds: [HandoffFd::Transfer],
    })
}

/// Borrow the frozen host-held creation material. No owned secret copies and no
/// alternative framing to the existing create handoff; caller zeroes every line.
pub fn encode_create_handoff(
    office_secrets_key: &OfficeSecretsKey,
    grant_key: &GrantKey,
    recovery_code: &RecoveryCode,
) -> Result<Vec<u8>, LaunchError> {
    #[derive(Serialize)]
    struct Create<'a> {
        op: &'static str,
        office_secrets_key: &'a OfficeSecretsKey,
        grant_key: &'a GrantKey,
        recovery_code: &'a RecoveryCode,
        fds: [HandoffFd; 1],
    }
    encode_secret(&Create {
        op: "create",
        office_secrets_key,
        grant_key,
        recovery_code,
        fds: [HandoffFd::Transfer],
    })
}

/// Borrow host-held secrets for a satellite join handoff; caller zeroes the returned line.
pub fn encode_join_handoff(
    office_secrets_key: &OfficeSecretsKey,
    grant_key: &GrantKey,
    invitation: &InvitationSecret,
) -> Result<Vec<u8>, LaunchError> {
    #[derive(Serialize)]
    struct Join<'a> {
        op: &'static str,
        office_secrets_key: &'a OfficeSecretsKey,
        grant_key: &'a GrantKey,
        invitation: &'a InvitationSecret,
        fds: [HandoffFd; 1],
    }
    encode_secret(&Join {
        op: "join",
        office_secrets_key,
        grant_key,
        invitation,
        fds: [HandoffFd::Transfer],
    })
}

/// The discard handoff: no secret, and `fds` is `[]` (no transfer socket, no marker follows).
/// It still goes only to the authenticated reader after `ready_for_secrets`, keeping the
/// handshake order of every op.
pub fn encode_discard_handoff() -> Result<Vec<u8>, LaunchError> {
    #[derive(Serialize)]
    struct Discard {
        op: &'static str,
        fds: [HandoffFd; 0],
    }
    encode_secret(&Discard {
        op: "discard",
        fds: [],
    })
}

pub(super) fn encode_secret(handoff: &impl Serialize) -> Result<Vec<u8>, LaunchError> {
    let body = Line {
        v: LAUNCH_VERSION,
        body: handoff,
    };
    // Measured first, so the buffer never reallocates and leaves a copy of a key behind.
    let mut counter = ByteCounter(0);
    serde_json::to_writer(&mut counter, &body).map_err(LaunchError::Json)?;
    let mut line = Vec::with_capacity(counter.0 + 1);
    if let Err(e) = serde_json::to_writer(&mut line, &body) {
        zero(&mut line);
        return Err(LaunchError::Json(e));
    }
    line.push(b'\n');
    Ok(line)
}

/// Decode a handoff line (without its `\n`). Another `v` is [`LaunchError::UnsupportedVersion`],
/// and a header naming the wrong descriptors is [`LaunchError::Invalid`]. The caller zeroes the
/// line afterwards.
pub fn decode_handoff(line: &[u8]) -> Result<Handoff, LaunchError> {
    check_version(line)?;
    let handoff: Handoff = serde_json::from_slice(line).map_err(LaunchError::Json)?;
    handoff.check()?;
    Ok(handoff)
}
