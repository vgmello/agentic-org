//! The mapping from the web process's ready line to the confinement it reports.

use org_host_control::web::{LandlockEnforcement, LandlockStatus, WebReadyForSecrets};
use org_platform::confinement::Enforcement;

use super::reported;

fn ready(landlock: Option<(u32, LandlockEnforcement)>) -> WebReadyForSecrets {
    WebReadyForSecrets {
        landlock: landlock.map(|(abi, enforcement)| LandlockStatus { abi, enforcement }),
    }
}

#[test]
fn a_line_without_landlock_reports_no_landlock_and_no_seccomp() {
    assert_eq!(
        reported(&ready(None)),
        Ok(Enforcement {
            landlock_abi: None,
            seccomp: false
        })
    );
}

#[test]
fn only_a_fully_enforced_domain_becomes_a_reported_abi() {
    assert_eq!(
        reported(&ready(Some((4, LandlockEnforcement::FullyEnforced)))),
        Ok(Enforcement {
            landlock_abi: Some(4),
            seccomp: false
        })
    );
    for partial in [
        LandlockEnforcement::PartiallyEnforced,
        LandlockEnforcement::NotEnforced,
    ] {
        assert_eq!(
            reported(&ready(Some((4, partial)))),
            Err("the ready line reports landlock not fully enforced"),
            "{partial:?}"
        );
    }
}
