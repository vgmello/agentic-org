use std::fmt;

use serde::de::{Error as _, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::Milliseconds;

/// Owned UTF-8 secret, redacted in diagnostics and wiped on drop. It is not Clone.
///
/// Decode directly from a zeroed-after-use input buffer into plain struct fields. As with
/// AppToken, this cannot wipe serde/HTTP buffers or copies made by the caller. Do not route
/// credentials through Value, flatten, untagged enums, URL parameters or logs.
///
/// ```compile_fail,E0599
/// let secret = org_proto::gateway::SecretText::new("public test value".into()).unwrap();
/// let _copy = secret.clone();
/// ```
pub struct SecretText(Vec<u8>);

impl SecretText {
    pub fn new(text: String) -> Result<Self, &'static str> {
        if text.is_empty() {
            return Err("a secret must be a nonempty string");
        }
        // Keep the String's allocation; shrinking into a box could leave a copied secret
        // in a freed allocation before Drop has a chance to wipe it.
        Ok(Self(text.into_bytes()))
    }

    pub fn expose(&self) -> &str {
        // Only UTF-8 strings enter, and the bytes are not mutable outside Drop.
        std::str::from_utf8(&self.0).expect("secret text is UTF-8")
    }
}

impl Drop for SecretText {
    fn drop(&mut self) {
        crate::token::zero(&mut self.0);
    }
}

impl fmt::Debug for SecretText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretText(<redacted>)")
    }
}

impl Serialize for SecretText {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.expose())
    }
}

impl<'de> Deserialize<'de> for SecretText {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct SecretVisitor;
        impl Visitor<'_> for SecretVisitor {
            type Value = SecretText;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a secret string")
            }
            fn visit_str<E: serde::de::Error>(self, text: &str) -> Result<Self::Value, E> {
                SecretText::new(text.to_owned()).map_err(E::custom)
            }
            fn visit_string<E: serde::de::Error>(self, text: String) -> Result<Self::Value, E> {
                SecretText::new(text).map_err(E::custom)
            }
            // Serde's default scalar errors print values. A mistyped credential must not leak.
            fn visit_bool<E: serde::de::Error>(self, _: bool) -> Result<Self::Value, E> {
                Err(E::custom("expected a secret string"))
            }
            fn visit_i64<E: serde::de::Error>(self, _: i64) -> Result<Self::Value, E> {
                Err(E::custom("expected a secret string"))
            }
            fn visit_u64<E: serde::de::Error>(self, _: u64) -> Result<Self::Value, E> {
                Err(E::custom("expected a secret string"))
            }
            fn visit_i128<E: serde::de::Error>(self, _: i128) -> Result<Self::Value, E> {
                Err(E::custom("expected a secret string"))
            }
            fn visit_u128<E: serde::de::Error>(self, _: u128) -> Result<Self::Value, E> {
                Err(E::custom("expected a secret string"))
            }
            fn visit_f64<E: serde::de::Error>(self, _: f64) -> Result<Self::Value, E> {
                Err(E::custom("expected a secret string"))
            }
        }
        d.deserialize_any(SecretVisitor)
    }
}

/// Independent 256-bit origin proof, encoded as 64 lowercase hex digits. Creation and
/// constant-time verification belong to B1-06's maintained authentication implementation.
///
/// ```compile_fail,E0599
/// let proof = org_proto::gateway::SessionProof::parse("00".repeat(32)).unwrap();
/// let _copy = proof.clone();
/// ```
pub struct SessionProof(SecretText);

impl SessionProof {
    pub fn parse(text: String) -> Result<Self, &'static str> {
        Self::from_secret(SecretText::new(text)?)
    }

    fn from_secret(text: SecretText) -> Result<Self, &'static str> {
        let bytes = text.expose().as_bytes();
        if bytes.len() != 64
            || !bytes
                .iter()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(b))
        {
            return Err("a session proof must be 64 lowercase hex digits");
        }
        Ok(Self(text))
    }

    pub fn expose(&self) -> &str {
        self.0.expose()
    }
}

impl fmt::Debug for SessionProof {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SessionProof(<redacted>)")
    }
}

impl Serialize for SessionProof {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(s)
    }
}

impl<'de> Deserialize<'de> for SessionProof {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Self::from_secret(SecretText::deserialize(d)?).map_err(D::Error::custom)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SignInParams {
    pub login: String,
    pub password: SecretText,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountView {
    pub id: String,
    pub login: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrganizationLink {
    pub org_id: String,
    pub member_id: String,
    pub office_ids: Vec<String>,
}

/// `GET session` never includes the proof. A cookie alone cannot recover one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionView {
    pub instance_id: String,
    pub account: AccountView,
    pub admin: bool,
    pub organizations: Vec<OrganizationLink>,
    pub server_time_ms: Milliseconds,
    pub absolute_expires_at: String,
    pub idle_expires_at: String,
}

/// Sign-in/refresh result. Fields stay flat on the wire, without serde flatten buffering a
/// secret. No host session handle, channel or grant appears in this public response.
#[derive(Debug, Serialize, Deserialize)]
pub struct SessionEstablished {
    pub instance_id: String,
    pub account: AccountView,
    pub admin: bool,
    pub organizations: Vec<OrganizationLink>,
    pub server_time_ms: Milliseconds,
    pub absolute_expires_at: String,
    pub idle_expires_at: String,
    pub proof: SessionProof,
}

impl SessionEstablished {
    pub fn new(view: SessionView, proof: SessionProof) -> Self {
        Self {
            instance_id: view.instance_id,
            account: view.account,
            admin: view.admin,
            organizations: view.organizations,
            server_time_ms: view.server_time_ms,
            absolute_expires_at: view.absolute_expires_at,
            idle_expires_at: view.idle_expires_at,
            proof,
        }
    }

    pub fn view(&self) -> SessionView {
        SessionView {
            instance_id: self.instance_id.clone(),
            account: self.account.clone(),
            admin: self.admin,
            organizations: self.organizations.clone(),
            server_time_ms: self.server_time_ms,
            absolute_expires_at: self.absolute_expires_at.clone(),
            idle_expires_at: self.idle_expires_at.clone(),
        }
    }
}
