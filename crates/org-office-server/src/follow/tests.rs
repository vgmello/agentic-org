use org_proto::directory::MemberPresence;

use super::*;

fn data(member: &str, status: &str) -> MemberPresenceChangedData {
    MemberPresenceChangedData {
        member_id: member.to_owned(),
        presence: MemberPresence::Offline,
        last_seen_at: "2026-11-09T12:00:00.000Z".to_owned(),
        status: Some(status.to_owned()),
        status_expires_at: None,
        current_task_id: None,
    }
}

#[test]
fn the_first_presence_goes_at_once_a_sooner_one_waits_and_the_latest_wins() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    let gap = Duration::from_secs(1);
    let mut gate = PresenceGate::new(gap);
    let t0 = Instant::now();
    assert_eq!(gate.offer(data("A", "one"), t0), Some(data("A", "one")));
    // Within the gap: held, and a newer one replaces it.
    assert_eq!(
        gate.offer(data("A", "two"), t0 + Duration::from_millis(100)),
        None
    );
    assert_eq!(
        gate.offer(data("A", "three"), t0 + Duration::from_millis(200)),
        None
    );
    assert!(gate.due(t0 + Duration::from_millis(999)).is_empty());
    assert_eq!(
        gate.next_due(t0 + Duration::from_millis(400)),
        Some(Duration::from_millis(600))
    );
    assert_eq!(gate.due(t0 + gap), [data("A", "three")]);
    // Sent again at t0+gap: the next gap starts there.
    assert_eq!(
        gate.offer(data("A", "four"), t0 + gap + Duration::from_millis(10)),
        None
    );
    assert_eq!(
        gate.next_due(t0 + gap + Duration::from_millis(10)),
        Some(Duration::from_millis(990))
    );
}

#[test]
fn members_are_gated_apart_and_one_after_the_gap_goes_at_once() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    let gap = Duration::from_secs(1);
    let mut gate = PresenceGate::new(gap);
    let t0 = Instant::now();
    assert!(gate.offer(data("A", "a"), t0).is_some());
    assert!(
        gate.offer(data("B", "b"), t0).is_some(),
        "another member is not held"
    );
    assert!(
        gate.offer(data("A", "a2"), t0 + gap).is_some(),
        "after the gap it goes now"
    );
    assert_eq!(gate.next_due(t0 + gap), None, "nothing waits");
    // A held event that a later one overtakes after the gap is not sent twice.
    assert!(gate
        .offer(data("B", "b2"), t0 + Duration::from_millis(10))
        .is_none());
    assert!(gate.offer(data("B", "b3"), t0 + gap).is_some());
    assert!(gate.due(t0 + gap * 2).is_empty());
}
