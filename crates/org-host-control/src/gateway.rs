//! The gateway's side of its rendezvous (X-02 §2, §5.1, §5.4): the lines on its launch pipe and on
//! the rendezvous connection before the control connection starts. Lines are JSON with `"v"`,
//! this contract's [`crate::API_VERSION`].
//!
//! The order: the host spawns the gateway suspended, checks its image, creates the rendezvous and
//! writes [`GatewayFirstLine`] on its stdin. The gateway applies its sandbox, connects, and sends
//! [`GatewayReadyForSecrets`]. After its post-accept and sandbox checks the host sends one
//! [`GatewayHandoff`] line, then one `sendmsg` carrying a 1-byte marker (`host.sock`'s,
//! [`crate::ops::HOST_SOCK_MARKER`]: this framing follows its rule) and exactly the two
//! descriptors it names: the transfer socket, then the browser listener. The same connection then
//! becomes the control connection. Transfer-socket records are `org-office-launch`'s v3
//! `TransferRecord`.

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::API_VERSION;

/// The gateway's first stdin line: non-secret. `Clone`, which no secret type is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayFirstLine {
    /// The absolute path of the per-spawn rendezvous socket, the only path the gateway's sandbox
    /// lets it connect to.
    pub rendezvous: String,
    pub instance_id: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FirstLineWire<T> {
    v: u32,
    rendezvous: T,
    instance_id: T,
}

impl Serialize for GatewayFirstLine {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        FirstLineWire {
            v: API_VERSION,
            rendezvous: &self.rendezvous,
            instance_id: &self.instance_id,
        }
        .serialize(s)
    }
}

impl<'de> Deserialize<'de> for GatewayFirstLine {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let wire = FirstLineWire::<String>::deserialize(d)?;
        check_version::<D>(wire.v)?;
        let plain = (1..=64).contains(&wire.instance_id.len())
            && wire.instance_id.bytes().all(|b| b.is_ascii_alphanumeric());
        if !plain {
            return Err(serde::de::Error::custom(
                "the instance id must be 1 to 64 ASCII letters and digits",
            ));
        }
        if !wire.rendezvous.starts_with('/') || wire.rendezvous.contains('\0') {
            return Err(serde::de::Error::custom(
                "the rendezvous path must be absolute, without NUL",
            ));
        }
        Ok(GatewayFirstLine {
            rendezvous: wire.rendezvous,
            instance_id: wire.instance_id,
        })
    }
}

fn check_version<'de, D: Deserializer<'de>>(v: u32) -> Result<(), D::Error> {
    if v == API_VERSION {
        Ok(())
    } else {
        Err(serde::de::Error::custom(
            "a gateway line of another version",
        ))
    }
}

/// The gateway's request for its secrets, its first line on the rendezvous connection:
/// `{"v": 1, "event": "ready_for_secrets"}`, with `landlock` on Linux. It is sent from the
/// gateway's only thread after its one seccomp filter is installed, before any network input, so
/// the host can trust the report as far as it trusts the image it checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GatewayReadyForSecrets {
    /// Linux only: the Landlock domain the gateway applied. Absent on macOS.
    pub landlock: Option<LandlockStatus>,
}

/// What the gateway reports about its Landlock domain. The host requires an ABI at or above its
/// minimum and full enforcement, and kills the gateway otherwise.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LandlockStatus {
    /// `landlock_create_ruleset(NULL, 0, LANDLOCK_CREATE_RULESET_VERSION)`.
    pub abi: u32,
    pub enforcement: LandlockEnforcement,
}

/// How fully the ruleset is enforced (the `landlock` crate's `RulesetStatus`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LandlockEnforcement {
    FullyEnforced,
    PartiallyEnforced,
    NotEnforced,
}

const READY_FOR_SECRETS: &str = "ready_for_secrets";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadyWire<E> {
    v: u32,
    event: E,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    landlock: Option<LandlockStatus>,
}

impl Serialize for GatewayReadyForSecrets {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        ReadyWire {
            v: API_VERSION,
            event: READY_FOR_SECRETS,
            landlock: self.landlock,
        }
        .serialize(s)
    }
}

impl<'de> Deserialize<'de> for GatewayReadyForSecrets {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let wire = ReadyWire::<String>::deserialize(d)?;
        check_version::<D>(wire.v)?;
        if wire.event != READY_FOR_SECRETS {
            return Err(serde::de::Error::custom("not ready_for_secrets"));
        }
        Ok(GatewayReadyForSecrets {
            landlock: wire.landlock,
        })
    }
}

/// A descriptor named by the [`GatewayHandoff`], in the order they arrive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GatewayFd {
    /// The gateway's end of its transfer socket (receive-only: the host shut down its write side).
    Transfer,
    /// The browser listener the host bound from the instance configuration.
    Listener,
}

/// The host's answer to [`GatewayReadyForSecrets`]: `{"v": 1, "fds": ["transfer", "listener"]}`.
/// It carries no key (the gateway's TLS pair comes from `tls.fetch`); it names the two
/// descriptors the following `sendmsg` carries, in order. Any other list is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GatewayHandoff;

/// The descriptors a [`GatewayHandoff`] names, in order.
pub const GATEWAY_HANDOFF_FDS: [GatewayFd; 2] = [GatewayFd::Transfer, GatewayFd::Listener];

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct HandoffWire {
    v: u32,
    fds: Vec<GatewayFd>,
}

impl Serialize for GatewayHandoff {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        HandoffWire {
            v: API_VERSION,
            fds: GATEWAY_HANDOFF_FDS.to_vec(),
        }
        .serialize(s)
    }
}

impl<'de> Deserialize<'de> for GatewayHandoff {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let wire = HandoffWire::deserialize(d)?;
        check_version::<D>(wire.v)?;
        if wire.fds != GATEWAY_HANDOFF_FDS {
            return Err(serde::de::Error::custom(
                "the gateway handoff must name the transfer socket, then the listener",
            ));
        }
        Ok(GatewayHandoff)
    }
}
