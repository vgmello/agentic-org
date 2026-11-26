//! Tests of the macOS adapter against the real `security` tool. They only look up an item
//! no one has, which never prompts and changes nothing.

use super::find_generic_password;
use std::time::Duration;

#[test]
fn a_missing_item_gives_no_answer() {
    let service = format!(
        "org-platform keychain test, missing item {}",
        std::process::id()
    );
    assert_eq!(
        find_generic_password(&service, 4096, Duration::from_secs(10))
            .map(|answer| answer.map(|bytes| bytes.as_slice().to_vec())),
        Ok(None)
    );
}
