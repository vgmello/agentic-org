//! Replicated journal kind checks.

use super::filters::is_replicated_op_kind;

#[test]
fn replicated_op_kinds_include_handoff_accept_without_ring_fallback() {
    for kind in [
        "organization.apply_replicated_files",
        "replication.apply_message",
        "replication.apply_conversation",
        "replication.apply_origin_close",
        "replication.apply_handoff_accept",
    ] {
        assert!(
            is_replicated_op_kind(kind),
            "missing replicated kind {kind}"
        );
    }
    assert!(!is_replicated_op_kind("messaging.accept_handoff"));
    assert!(!is_replicated_op_kind("messaging.send"));
}
