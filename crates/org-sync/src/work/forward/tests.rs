use super::*;

#[test]
fn only_ahead_and_conflicting_ack_faults_require_connection_close() {
    for code in [Refusal::AheadOfForwarded, Refusal::ConflictingProof] {
        assert!(ack_closes_connection(&code.into()));
    }
    for code in [
        Refusal::Gap,
        Refusal::Malformed,
        Refusal::WrongStream,
        Refusal::NotAuthorized,
    ] {
        assert!(!ack_closes_connection(&code.into()));
    }
    assert!(SyncError::from(Refusal::ConflictingProof).aborts_batch());
}
