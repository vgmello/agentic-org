use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Weak};

use org_office_launch::recovery::Request;

use super::*;
use crate::hostdb::recovery::State;

fn ledger(ordinal: i64) -> Ledger {
    Ledger {
        version: 1,
        binding: Request {
            version: 1,
            instance_id: "instance".into(),
            office_id: "office".into(),
            organization_id: "org".into(),
            operation_id: "operation".into(),
        },
        public_key: "key".into(),
        package_sha256: "sha".into(),
        state: State::Delivered,
        ordinal,
        first_shown_ms: Some(1),
        last_redelivered_ms: None,
        redelivery_count: 0,
        acknowledged_ms: None,
        acknowledged_by: None,
    }
}

#[test]
fn a_permission_admits_only_its_own_live_session_account_and_delivery() {
    let alive = Arc::new(AtomicBool::new(true));
    let live = Arc::downgrade(&alive);
    let permission = Permission::new([7; 32], live.clone(), "owner".into(), ledger(1));
    assert!(permission.admits("owner", &[7; 32], &live, &ledger(1)));

    // Every bound fact refuses on its own.
    assert!(!permission.admits("other", &[7; 32], &live, &ledger(1)));
    assert!(!permission.admits("owner", &[8; 32], &live, &ledger(1)));
    assert!(!permission.admits("owner", &[7; 32], &live, &ledger(2)));
    let other_binding = Arc::new(AtomicBool::new(true));
    assert!(!permission.admits(
        "owner",
        &[7; 32],
        &Arc::downgrade(&other_binding),
        &ledger(1)
    ));
    assert!(!permission.admits("owner", &[7; 32], &Weak::new(), &ledger(1)));

    // A session that ended, or whose binding is gone, revokes it in place.
    alive.store(false, Ordering::SeqCst);
    assert!(!permission.admits("owner", &[7; 32], &live, &ledger(1)));
    drop(alive);
    assert!(!permission.admits("owner", &[7; 32], &live, &ledger(1)));
}
