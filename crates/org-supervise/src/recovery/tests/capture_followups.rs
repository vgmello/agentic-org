use super::*;
use crate::recovery::capture::{capture_targets, CaptureScope, ValidatedSnapshot};

fn capture(
    ops: &mut Processes,
    saved: &[CheckpointTarget],
    leader: Option<&ChildIdentity>,
) -> Vec<VerifiedTarget> {
    let deadline = ops.deadline();
    let snapshot = ValidatedSnapshot::capture(ops, deadline).unwrap();
    let context = ops.context.clone();
    capture_targets(
        CaptureScope {
            mark: MARK,
            excluded: None,
            saved,
            leader,
        },
        &snapshot,
        &context,
        ops,
        deadline,
        &mut RecoveryReport::default(),
    )
    .unwrap()
}

#[test]
fn recovery_capture_preserves_saved_ancestry_over_optional_job_proof() {
    let member = unreadable_job_member(200, 100, 100);
    let saved = CheckpointTarget {
        identity: member.identity.clone().unwrap(),
        ancestry: vec![process(150, 1, MarkEvidence::Absent).identity.unwrap()],
    };
    let record = record(true);
    // No current parent link explains the member; its historical ancestry matters.
    let mut ops = Processes::new(vec![marked(100), member]);
    let targets = capture(&mut ops, std::slice::from_ref(&saved), record.leader());
    assert_eq!(targets.len(), 2);
    let target = targets.iter().find(|t| t.identity.pid == 200).unwrap();
    assert_eq!(target.identity, saved.identity);
    match target.evidence() {
        TargetEvidence::Checkpoint(ancestry) => assert_eq!(ancestry, &saved.ancestry),
        evidence => panic!("saved authority was replaced: {evidence:?}"),
    }
    assert!(ops.job_verifications.is_empty());

    // The identical live member is job eligible when it has no saved authority.
    let targets = capture(&mut ops, &[], record.leader());
    assert_eq!(ops.job_verifications, [saved.identity]);
    assert!(targets.iter().any(|t| t.identity.pid == 200
        && matches!(t.evidence(), TargetEvidence::Checkpoint(a) if a.is_empty())));
}

#[test]
fn recovery_capture_traverses_job_anchor_and_unreadable_intermediate() {
    let anchor = unreadable_job_member(200, 100, 100);
    let direct = process(201, 200, MarkEvidence::Absent);
    // This unreadable link is outside the recorded job and is not a signal target.
    let intermediate = process(202, 200, MarkEvidence::Unreadable);
    let leaf = process(203, 202, MarkEvidence::Present("other-session".into()));
    let anchor_id = anchor.identity.clone().unwrap();
    let intermediate_id = intermediate.identity.clone().unwrap();
    let record = record(true);
    let mut ops = Processes::new(vec![
        process(100, 1, MarkEvidence::Absent),
        anchor,
        direct,
        intermediate,
        leaf,
    ]);
    let targets = capture(&mut ops, &[], record.leader());
    assert_eq!(
        ops.job_verifications.as_slice(),
        std::slice::from_ref(&anchor_id)
    );
    assert_eq!(
        targets.iter().map(|t| t.identity.pid).collect::<Vec<_>>(),
        [200, 201, 203]
    );
    assert!(matches!(targets[0].evidence(), TargetEvidence::Checkpoint(a) if a.is_empty()));
    for (pid, expected) in [
        (201, vec![anchor_id.clone()]),
        (203, vec![anchor_id, intermediate_id]),
    ] {
        let target = targets.iter().find(|t| t.identity.pid == pid).unwrap();
        match target.evidence() {
            TargetEvidence::CheckpointDescendant(ancestry) => assert_eq!(ancestry, &expected),
            evidence => panic!("job anchor must supply checkpoint authority: {evidence:?}"),
        }
    }

    ops.job_error = true;
    assert!(capture(&mut ops, &[], record.leader()).is_empty());
    assert!(capture(&mut ops, &[], None).is_empty());
    assert!(ops.signals().is_empty());
}

#[test]
fn recovery_capture_filters_unsafe_identity_before_optional_job_verification() {
    let record = record(true);
    for variant in 0..11 {
        let mut member = unreadable_job_member(200, 100, 100);
        let mut ops = Processes::new(vec![]);
        match variant {
            0 => {} // Positive control reaches the verifier and becomes a target.
            1 => member.identity = None,
            2 => member.identity.as_mut().unwrap().started.clear(),
            3 => member.identity.as_mut().unwrap().boot.clear(),
            4 => member.identity.as_mut().unwrap().pid = 201,
            5 => member.identity.as_mut().unwrap().boot = "other-boot".into(),
            6 => ops.context.pid = member.pid,
            7 => member.credentials_safe = false,
            8 => {
                member.uid = 0;
                ops.context.uid = 0;
            }
            9 => member.uid = 502,
            10 => {
                member.pid = 1;
                member.identity.as_mut().unwrap().pid = 1;
            }
            _ => unreachable!(),
        }
        ops.live = vec![process(100, 1, MarkEvidence::Absent), member];
        let targets = capture(&mut ops, &[], record.leader());
        if variant == 0 {
            assert_eq!(ops.job_verifications, [targets[0].identity.clone()]);
            assert_eq!(targets.len(), 1);
            assert_eq!(targets[0].identity.pid, 200);
        } else {
            assert!(ops.job_verifications.is_empty(), "variant {variant}");
            assert!(targets.is_empty(), "variant {variant}");
        }
        assert!(ops.signals().is_empty());
    }
}
