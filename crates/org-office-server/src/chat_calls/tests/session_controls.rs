//! A person's live turn controls (`TASK-103`) through the office's full dispatcher, as the
//! office socket and the host channel serve them: `member.session_interrupt` and
//! `member.session_steer` reach the session manager (never `unknown_method`), which lets the
//! owner or any active person control an ordinary agent's turn, only the owner control the Chief
//! of Staff's, and no agent control any turn.

use super::*;
use org_proto::sessions::{
    MemberSessionInterrupt, MemberSessionInterruptParams, MemberSessionSteer,
    MemberSessionSteerParams,
};

fn interrupt(seed: u16, target: &str) -> Request {
    Request::new::<MemberSessionInterrupt>(
        u64::from(seed),
        &MemberSessionInterruptParams {
            command_id: uuidv7(seed),
            employee_id: target.to_owned(),
        },
    )
    .unwrap()
}

fn steer(seed: u16, target: &str) -> Request {
    Request::new::<MemberSessionSteer>(
        u64::from(seed),
        &MemberSessionSteerParams {
            command_id: uuidv7(seed),
            employee_id: target.to_owned(),
            text: "Use the staging database instead".into(),
        },
    )
    .unwrap()
}

/// The error code `caller` gets (a person through a grant, an agent through its own binding).
fn code(fx: &LiveOfficeFixture, request: &Request, caller: &str, via_grant: bool) -> ErrorCode {
    match fx
        .shared
        .answer_for_test(request, caller, via_grant)
        .outcome
    {
        Outcome::Error(error) => error.code,
        Outcome::Result(value) => panic!("{} succeeded: {value}", request.method),
    }
}

#[test]
fn task103_turn_controls_reach_the_session_manager_under_the_person_and_tess_policy() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    let fx = LiveOfficeFixture::start();
    let tess = fx
        .office
        .chief_of_staff()
        .unwrap()
        .expect("Tess")
        .id
        .to_string();
    let top_team = fx.office.org_snapshot().unwrap().teams[0].id.clone();
    let bob = uuidv7(120);
    let add_bob = Request::new::<org_proto::org::MemberAddPerson>(
        120,
        &org_proto::org::MemberAddPersonParams {
            command_id: uuidv7(121),
            member_id: bob.clone(),
            name: "Bob".into(),
            contact: None,
            handle: Some("bob".into()),
            home_team_id: Some(top_team),
        },
    )
    .unwrap();
    assert!(matches!(
        fx.shared
            .answer_for_test(&add_bob, &fx.owner_id, true)
            .outcome,
        Outcome::Result(_)
    ));

    for (n, control) in [interrupt as fn(u16, &str) -> Request, steer]
        .into_iter()
        .enumerate()
    {
        let seed = 130 + 10 * u16::try_from(n).unwrap();
        let on_hr = control(seed, &fx.hr_id);
        let on_tess = control(seed + 1, &tess);
        let method = on_hr.method.clone();
        // Authorized and dispatched: the agent has no live session to control, so `conflict`
        // (before the fix the office answered `unknown_method` and the web BFF 503).
        assert_eq!(
            code(&fx, &on_hr, &fx.owner_id, true),
            ErrorCode::Conflict,
            "{method}: owner on an ordinary agent"
        );
        assert_eq!(
            code(&fx, &on_hr, &bob, true),
            ErrorCode::Conflict,
            "{method}: a non-owner person on an ordinary agent"
        );
        assert_eq!(
            code(&fx, &on_tess, &fx.owner_id, true),
            ErrorCode::Conflict,
            "{method}: the owner on the Chief of Staff"
        );
        // Tess serves the owner's private partitions: no other person controls her turns.
        assert_eq!(
            code(&fx, &on_tess, &bob, true),
            ErrorCode::NotAuthorized,
            "{method}: a non-owner person on the Chief of Staff"
        );
        // An agent never controls a turn: another agent's, Tess's, or its own.
        for target in [&on_hr, &on_tess] {
            assert_eq!(
                code(&fx, target, &tess, false),
                ErrorCode::NotAuthorized,
                "{method}: an agent caller"
            );
        }
        assert_eq!(
            code(&fx, &on_hr, &fx.hr_id, false),
            ErrorCode::NotAuthorized,
            "{method}: an agent on its own turn"
        );
    }
}
