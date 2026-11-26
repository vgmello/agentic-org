//! On a target without a keychain adapter, the public function refuses.

use super::find_generic_password;
use crate::Refused;
use std::time::Duration;

#[test]
fn this_target_refuses_the_keychain() {
    assert_eq!(
        find_generic_password("any service", 4096, Duration::from_secs(1))
            .map(|answer| answer.map(|bytes| bytes.as_slice().to_vec())),
        Err(Refused::Unsupported {
            capability: "keychain",
            os: std::env::consts::OS,
        })
    );
}
