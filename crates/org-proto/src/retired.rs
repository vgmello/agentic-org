//! Retired input fields (X-02 §4.3): fields a client may no longer send, refused by a permanent
//! sentinel rather than silently ignored.
//!
//! A retired field has the type `Option<Retired>` and is decoded **on presence** with one of the
//! functions here, so any value is refused, `null` included, and an absent field is `None`. A
//! type that is also written marks it `skip_serializing`, so it never appears on the wire:
//!
//! ```
//! # use serde::Deserialize;
//! # use org_proto::retired::{self, Retired};
//! #[derive(Deserialize)]
//! struct Params {
//!     #[serde(default, deserialize_with = "retired::home")]
//!     home: Option<Retired>,
//! }
//! assert!(serde_json::from_str::<Params>("{}").is_ok());
//! assert!(serde_json::from_str::<Params>(r#"{"home": null}"#).is_err());
//! ```
//!
//! The refusal happens while the params decode, so a server answers it with `invalid_request`
//! before any rule runs or any file is touched (SRV-29). The sentinels stay after the migration's
//! move 3: an old client gets a clear answer instead of a silent no-op.

use serde::de::Error as _;
use serde::Deserializer;

/// The message for a retired home field (`home`, `clear_home`, `profile.home`).
pub const HOME_RETIRED: &str = "home is managed by the office";

/// The message for the retired `app_token` field.
pub const APP_TOKEN_RETIRED: &str = "app_token is retired";

/// The type of a retired field: it has no values, so a decoded `Option<Retired>` is always
/// `None`. Use it only with the decoders in this module, which refuse every present value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Retired {}

/// Refuses a present home field (`home`, `clear_home`, `profile.home`), whatever its value.
pub fn home<'de, D: Deserializer<'de>>(_: D) -> Result<Option<Retired>, D::Error> {
    Err(D::Error::custom(HOME_RETIRED))
}

/// Refuses a present `app_token`, whatever its value. The value is never read, so it can't
/// appear in the error.
pub fn app_token<'de, D: Deserializer<'de>>(_: D) -> Result<Option<Retired>, D::Error> {
    Err(D::Error::custom(APP_TOKEN_RETIRED))
}
