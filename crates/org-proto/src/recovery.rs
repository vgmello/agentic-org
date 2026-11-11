//! The recovery code secret type shared by `org-host-control` and `org-office-launch`.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::gateway::SecretText;

/// An organization's recovery code (X-02 §6.4): staged by the host, sent to the office server in
/// launch v3's create handoff (`org-office-launch`) and shown once by `recovery.release`
/// (`org-host-control`), printed only to a TTY, never in JSON status output or logs. It isn't part
/// of the browser contract: it lives in its own module, built on [`SecretText`], so that both crates
/// re-export one type with one set of guards. It decodes from a JSON string (a non-empty one), its `Debug`
/// is redacted, it isn't `Clone`, and its bytes are zeroed on drop. Decode errors never quote it.
///
/// ```compile_fail,E0599
/// let code = org_proto::RecoveryCode::new("ABCD-EFGH-IJKL".into()).unwrap();
/// let _copy = code.clone();
/// ```
#[derive(Serialize, Deserialize)]
#[serde(transparent)]
pub struct RecoveryCode(SecretText);

impl RecoveryCode {
    /// Wrap a non-empty code.
    pub fn new(code: String) -> Result<Self, &'static str> {
        SecretText::new(code).map(RecoveryCode)
    }

    /// The code, for the one use it has (the escrow, or the TTY). Never log or format it.
    pub fn expose(&self) -> &str {
        self.0.expose()
    }
}

impl fmt::Debug for RecoveryCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("RecoveryCode(<redacted>)")
    }
}
