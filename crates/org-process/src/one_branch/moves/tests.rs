//! §4.2's own work and guarded moves, table-driven (processes.md Appendix C "One branch" and
//! "Guarded moves"): each case fails without its guard.

use super::super::TaskRef;
use super::*;
use Tip::{AtBase, Beyond, Integrated, Missing, Unreadable, Unrelated};

/// A branch and HEAD at the base, no `own_tip` recorded and never pushed.
const FRESH: Tips = Tips {
    branch: AtBase,
    head: AtBase,
    own_tip: Missing,
    remote: Missing,
};

/// `own_tip` merged with a `--no-ff` merge, the remote head with it, the branch and HEAD back at
/// the base.
const MERGED: Tips = Tips {
    own_tip: Integrated,
    remote: Integrated,
    ..FRESH
};

#[test]
fn a_judged_sha_is_at_the_base_first_then_in_integration_then_beyond() {
    let cases = [
        ("at the base, whatever else", (true, true, true), AtBase),
        (
            "at the base, not in integration",
            (true, false, false),
            AtBase,
        ),
        ("in integration", (false, true, true), Integrated),
        (
            "in integration, not from the base",
            (false, false, true),
            Integrated,
        ),
        (
            "from the base, not in integration",
            (false, true, false),
            Beyond,
        ),
        ("neither", (false, false, false), Unrelated),
    ];
    for (name, (at_base, descends, in_integration), expected) in cases {
        assert_eq!(
            Tip::judge(at_base, descends, in_integration),
            expected,
            "{name}"
        );
    }
}

#[test]
fn own_work_is_none_merged_or_some() {
    let cases: Vec<(&str, Tips, bool, Found)> = vec![
        ("fresh: no own_tip, never pushed", FRESH, false, Found::None),
        ("uncommitted changes", FRESH, true, Found::Some),
        (
            "tips in integration, own_tip not recorded",
            Tips {
                branch: Integrated,
                head: Integrated,
                remote: Integrated,
                ..FRESH
            },
            false,
            Found::None,
        ),
        (
            "a reset onto someone else's branch, no own_tip: fails closed",
            Tips {
                branch: Unrelated,
                head: Unrelated,
                remote: Unrelated,
                ..FRESH
            },
            false,
            Found::Some,
        ),
        (
            "only the HEAD copy unrelated, no own_tip: fails closed",
            Tips {
                head: Unrelated,
                ..FRESH
            },
            false,
            Found::Some,
        ),
        (
            "missing branch",
            Tips {
                branch: Missing,
                ..FRESH
            },
            false,
            Found::Some,
        ),
        (
            "missing HEAD",
            Tips {
                head: Missing,
                ..FRESH
            },
            false,
            Found::Some,
        ),
        (
            "unreadable branch",
            Tips {
                branch: Unreadable,
                ..FRESH
            },
            false,
            Found::Some,
        ),
        (
            "unreadable HEAD",
            Tips {
                head: Unreadable,
                ..FRESH
            },
            false,
            Found::Some,
        ),
        (
            "unreadable own_tip",
            Tips {
                own_tip: Unreadable,
                ..FRESH
            },
            false,
            Found::Some,
        ),
        (
            "unreadable remote head",
            Tips {
                remote: Unreadable,
                ..FRESH
            },
            false,
            Found::Some,
        ),
        (
            "a commit on the branch",
            Tips {
                branch: Beyond,
                ..FRESH
            },
            false,
            Found::Some,
        ),
        (
            "a commit on a detached HEAD",
            Tips {
                head: Beyond,
                ..FRESH
            },
            false,
            Found::Some,
        ),
        (
            "own_tip beyond, branch reset to the base",
            Tips {
                own_tip: Beyond,
                ..FRESH
            },
            false,
            Found::Some,
        ),
        (
            "pushed, then reset",
            Tips {
                remote: Beyond,
                ..FRESH
            },
            false,
            Found::Some,
        ),
        ("own_tip merged", MERGED, false, Found::Merged),
        (
            "own_tip merged, never pushed, branch in integration",
            Tips {
                branch: Integrated,
                remote: Missing,
                ..MERGED
            },
            false,
            Found::Merged,
        ),
        (
            "own_tip merged, uncommitted changes",
            MERGED,
            true,
            Found::Some,
        ),
        (
            "own_tip merged, a later local commit",
            Tips {
                head: Beyond,
                ..MERGED
            },
            false,
            Found::Some,
        ),
        (
            "own_tip merged, the branch beyond it on someone else's work",
            Tips {
                branch: Unrelated,
                ..MERGED
            },
            false,
            Found::Some,
        ),
        (
            "own_tip merged, the remote head beyond it on someone else's work",
            Tips {
                remote: Unrelated,
                ..MERGED
            },
            false,
            Found::Some,
        ),
        (
            "own_tip merged, branch missing",
            Tips {
                branch: Missing,
                ..MERGED
            },
            false,
            Found::Some,
        ),
    ];
    for (name, tips, uncommitted, expected) in cases {
        assert_eq!(own_work(&tips, uncommitted).found(), expected, "{name}");
    }
}

/// Moved to a state, over own work or not; refused with `one_branch_open` naming the branch, task
/// and stage; or `not_authorized`.
#[derive(Debug, PartialEq)]
enum Answer {
    Moved(State, bool),
    Open(String, String, Option<String>),
    NotAuthorized,
}

fn record() -> Record {
    Record {
        task: TaskRef("chk-T-17".into()),
        branch: "task/chk-T-17-fix-login".into(),
        state: State::Closing,
        stage: Some("waiting for review".into()),
    }
}

fn answer(caller: Caller, found: Found) -> Answer {
    match guarded_move(caller, &OwnWork(found), &record()) {
        Ok(done) => Answer::Moved(done.state(), done.over_own_work()),
        Err(Refused::OneBranchOpen(open)) => Answer::Open(
            open.branch().into(),
            open.task().0.clone(),
            open.stage().map(Into::into),
        ),
        Err(Refused::NotAuthorized) => Answer::NotAuthorized,
    }
}

#[test]
fn a_guarded_move_over_own_work_needs_a_lead_who_isnt_the_assignee_or_the_owner() {
    let who = |role, assignee| Caller { role, assignee };
    let open = || {
        Answer::Open(
            "task/chk-T-17-fix-login".into(),
            "chk-T-17".into(),
            Some("waiting for review".into()),
        )
    };
    let over = || Answer::Moved(State::Archived, true);
    let cases = [
        ("the assignee", who(Role::Other, true), open()),
        ("a lead who is the assignee", who(Role::Lead, true), open()),
        ("a lead", who(Role::Lead, false), over()),
        ("the owner", who(Role::Owner, false), over()),
        (
            "the owner on their own task",
            who(Role::Owner, true),
            over(),
        ),
        (
            "anyone else",
            who(Role::Other, false),
            Answer::NotAuthorized,
        ),
    ];
    for (name, caller, expected) in cases {
        assert_eq!(answer(caller, Found::Some), expected, "{name}");
    }
}

#[test]
fn a_guarded_move_without_own_work_goes_through_for_anyone() {
    for role in [Role::Owner, Role::Lead, Role::Other] {
        for assignee in [true, false] {
            let caller = Caller { role, assignee };
            let archived = Answer::Moved(State::Archived, false);
            assert_eq!(answer(caller, Found::None), archived, "{caller:?}");
            let merged = Answer::Moved(State::Merged, false);
            assert_eq!(answer(caller, Found::Merged), merged, "{caller:?}");
        }
    }
}
