//! The secret newtypes of this contract (X-02 §6.5). Each wraps `org-proto`'s [`SecretText`]: it
//! decodes from a JSON string, its `Debug` is redacted, it isn't `Clone`, and its bytes are zeroed
//! on drop. Decode errors never quote the value. Zeroing covers this copy only: on the wire every
//! one of them also passes through the envelope's raw `serde_json::Value` (a request's `params`,
//! a response's result; §6.5's raw-JSON exception, see the crate docs), and copies in serde's and
//! the transport's buffers survive; see `org_proto`'s `token` module docs.

use org_proto::web::SecretText;
use serde::{Deserialize, Serialize};

macro_rules! secret_text {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(SecretText);

        impl $name {
            /// Wrap a non-empty secret.
            pub fn new(text: String) -> Result<Self, &'static str> {
                SecretText::new(text).map($name)
            }

            /// The secret, for the one use it has. Never log or format it.
            pub fn expose(&self) -> &str {
                self.0.expose()
            }
        }

        impl std::fmt::Debug for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(concat!(stringify!($name), "(<redacted>)"))
            }
        }
    };
}

secret_text!(
    /// A password, as typed: browser sign-in, registration, set-password, password change,
    /// password-confirmed handovers, and `account.verify`. The host keeps only its Argon2id hash;
    /// never the password, a fast hash or a separate key.
    Password
);

secret_text!(
    /// A sign-in session's handle: an opaque 256-bit value the host issues from
    /// `account.verify`, and the only way a host op names its actor. Distinct from the
    /// non-secret `session_id` that grants and revocations carry.
    SessionHandle
);

secret_text!(
    /// The TLS private key (PEM), in `tls.fetch`'s result and `tls.set`'s params. The host keeps it
    /// in its credential provider, never on disk in the clear.
    TlsPrivateKey
);

secret_text!(
    /// A one-time link's full URL (`<origin>/register#<token>`), in `admin.link.initial`'s and
    /// `admin.reset`'s results. The part after `#` is the secret; the host keeps only its digest.
    OneTimeLinkUrl
);
secret_text!(
    /// A one-time link's secret: the part after `#`, which the web process forwards to the host
    /// in `link.register`. The host keeps only its digest.
    LinkToken
);

secret_text!(
    /// A satellite office invitation token (`agorg-inv1.<payload>`) carrying the one-time
    /// enrollment private key (`doc-67`, `TASK-52`). Redacted in `Debug`, zeroed on drop.
    InvitationTokenSecret
);
