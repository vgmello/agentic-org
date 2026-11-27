//! Engine-independent `OneTimeLinks` suite (ADR-0018 ENG-3 for host storage).
//! Runs all 16 shared conformance cases against `MemoryHostStore` and `MemoryLinks` in
//! `org-host`; `org-host-sqlite/tests/conformance.rs` runs the exact same 16 cases against
//! SQLite `HostDb`.

use super::memory::MemoryLinks;
use super::OneTimeLinks;
use crate::hostdb::memory::MemoryHostStore;
use crate::test_support::conformance::links::{self, INSTANCE, NOW, OTHER_INSTANCE};

/// Runs `case` against both in-memory adapters for `INSTANCE`.
fn each(case: impl Fn(&dyn OneTimeLinks)) {
    case(&MemoryHostStore::new(INSTANCE, NOW));
    case(&MemoryLinks::new(INSTANCE));
}

#[test]
fn issued_link_redeems_once_then_is_used() {
    each(links::issued_link_redeems_once_then_is_used);
}

#[test]
fn issue_first_issues_once_per_subject_ever_and_a_replay_has_no_secret() {
    each(links::issue_first_issues_once_per_subject_ever_and_a_replay_has_no_secret);
}

#[test]
fn issuing_again_cancels_the_pending_link_of_the_same_subject_only() {
    each(links::issuing_again_cancels_the_pending_link_of_the_same_subject_only);
}

#[test]
fn a_link_expires_after_24_hours_by_the_injected_clock() {
    each(links::a_link_expires_after_24_hours_by_the_injected_clock);
}

#[test]
fn would_redeem_reports_every_state_without_changing_the_link() {
    each(links::would_redeem_reports_every_state_without_changing_the_link);
}

#[test]
fn unknown_and_malformed_tokens_are_unknown() {
    each(links::unknown_and_malformed_tokens_are_unknown);
}

#[test]
fn the_digest_is_bound_to_the_instance_and_kind() {
    links::the_digest_is_bound_to_the_instance_and_kind(
        &MemoryHostStore::new(INSTANCE, NOW),
        &MemoryHostStore::new(OTHER_INSTANCE, NOW),
    );
    links::the_digest_is_bound_to_the_instance_and_kind(
        &MemoryLinks::new(INSTANCE),
        &MemoryLinks::new(OTHER_INSTANCE),
    );
}

#[test]
fn subjects_are_bounded_printable_identifiers() {
    links::subjects_are_bounded_printable_identifiers();
}

#[test]
fn token_parse_refuses_wrong_lengths_before_decoding() {
    links::token_parse_refuses_wrong_lengths_before_decoding();
}

#[test]
fn token_parse_refuses_every_noncanonical_final_character() {
    links::token_parse_refuses_every_noncanonical_final_character();
}

#[test]
fn encoded_token_text_is_zeroized_on_drop() {
    links::encoded_token_text_is_zeroized_on_drop();
}

#[test]
fn token_is_zeroized_on_drop() {
    links::token_is_zeroized_on_drop();
}

#[test]
fn digest_fields_have_explicit_boundaries() {
    links::digest_fields_have_explicit_boundaries();
}

#[test]
fn token_text_round_trips_and_debug_hides_it() {
    links::token_text_round_trips_and_debug_hides_it();
}

#[test]
fn a_failed_replacement_keeps_the_previous_pending_link() {
    each(links::a_failed_replacement_keeps_the_previous_pending_link);
}

#[test]
fn peek_link_subject_reports_subject_only_when_pending_and_not_expired() {
    each(links::peek_link_subject_reports_subject_only_when_pending_and_not_expired);
}
