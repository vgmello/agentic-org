use super::*;
use crate::key::Key;
use crate::store::{Opening, Store};
use org_proto::org::TeamCreateParams;
use tempfile::TempDir;

const CLOCK: u64 = 1_800_000_000_000;
const OWNER: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";

fn setup() -> (TempDir, Store, OrgCommand) {
    let dir = TempDir::new().unwrap();
    let store = Store::open(
        &dir.path().join("org.db"),
        &Key::from_bytes([41; 32]),
        Opening::New,
        &SCHEMA,
    )
    .unwrap();
    store
        .write(|db| {
            genesis(
                db,
                &Genesis {
                    org_id: "ORG1".into(),
                    org_name: "Acme".into(),
                    created_at: time::format_epoch_ms(CLOCK),
                    owner_id: OWNER.into(),
                },
            )?;
            Ok(((), None))
        })
        .unwrap();
    let command = OrgCommand::TeamCreate(TeamCreateParams {
        command_id: ulid::from_ms(CLOCK, 1),
        team_id: "T1".into(),
        name: "Engineering".into(),
        owner_team_id: None,
    });
    (dir, store, command)
}

fn submit_on(store: &Store, command: OrgCommand, now: u64) -> Result<CommandAccepted, ApiError> {
    store
        .write(move |db| submit(db, now, &command).map(|answer| (answer, None)))
        .unwrap()
}

fn get(
    store: &Store,
    now: u64,
    id: &str,
    principal: Option<Principal<'_>>,
) -> Result<OperationOutcome, ApiError> {
    store.read(|db| lookup(db, now, id, principal)).unwrap()
}

fn image(store: &Store) -> (Vec<Vec<serde_json::Value>>, Option<String>) {
    store.read(|db| {
        let mut query = db.prepare("SELECT command_id,epoch,seq,expires_at,result,principal_account,principal_member,actor_kind FROM commands ORDER BY command_id").map_err(OfficeError::storage)?;
        let rows = query.query_map([], |row| {
            Ok(vec![serde_json::json!(row.get::<_,String>(0)?),serde_json::json!(row.get::<_,i64>(1)?),serde_json::json!(row.get::<_,i64>(2)?),serde_json::json!(row.get::<_,String>(3)?),serde_json::json!(row.get::<_,Option<String>>(4)?),serde_json::json!(row.get::<_,Option<String>>(5)?),serde_json::json!(row.get::<_,Option<String>>(6)?),serde_json::json!(row.get::<_,Option<String>>(7)?)])
        }).map_err(OfficeError::storage)?.collect::<rusqlite::Result<Vec<_>>>().map_err(OfficeError::storage)?;
        let high = db.query_row("SELECT value FROM meta WHERE key='clock_high_ms'", [], |row| row.get(0)).optional().map_err(OfficeError::storage)?;
        Ok((rows, high))
    }).unwrap()
}

fn principal() -> Principal<'static> {
    Principal {
        account: Some("A1"),
        member: Some(OWNER),
        kind: "human",
    }
}

#[test]
fn org_lookup_retains_commit_refusal_and_bytes_after_reopen() {
    let (dir, store, command) = setup();
    let id = command.command_id().to_owned();
    let accepted = submit_on(&store, command, CLOCK).unwrap();
    let refused_id = ulid::from_ms(CLOCK, 2);
    let refused = submit_on(
        &store,
        OrgCommand::TeamCreate(TeamCreateParams {
            command_id: refused_id.clone(),
            team_id: "T2".into(),
            name: " ".into(),
            owner_team_id: None,
        }),
        CLOCK,
    )
    .unwrap_err();
    let before = image(&store);
    let committed = get(&store, CLOCK, &id, None).unwrap();
    assert_eq!(
        serde_json::to_value(&committed).unwrap(),
        serde_json::json!({"status":"committed","result":accepted})
    );
    let rejected = get(&store, CLOCK, &refused_id, None).unwrap();
    assert_eq!(
        serde_json::to_value(&rejected).unwrap(),
        serde_json::json!({"status":"rejected","refusal":refused})
    );
    assert_eq!(image(&store), before);
    drop(store);
    let reopened = Store::open(
        &dir.path().join("org.db"),
        &Key::from_bytes([41; 32]),
        Opening::Existing,
        &SCHEMA,
    )
    .unwrap();
    assert_eq!(get(&reopened, CLOCK, &id, None).unwrap(), committed);
    assert_eq!(get(&reopened, CLOCK, &refused_id, None).unwrap(), rejected);
    assert_eq!(image(&reopened), before);
}

#[test]
fn malformed_ids_precede_lookup_and_absent_old_ids_are_not_found() {
    let (_dir, store, _) = setup();
    for id in [
        "",
        "bad",
        "01arz3ndektsv4rrffq69g5fav",
        "81ARZ3NDEKTSV4RRFFQ69G5FAV",
    ] {
        assert_eq!(
            get(&store, CLOCK, id, None).unwrap_err().code,
            ErrorCode::InvalidRequest
        );
    }
    assert_eq!(
        get(&store, CLOCK, "00000000000000000000000000", None)
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
    assert_eq!(image(&store).0.len(), 0);
    assert_eq!(image(&store).1, None);
}

#[test]
fn expiry_uses_retained_instant_inclusive_boundary_and_durable_clock() {
    let (_dir, store, command) = setup();
    let id = command.command_id().to_owned();
    submit_on(&store, command, CLOCK).unwrap();
    for (delta, expired) in [(-1, true), (0, true), (1, false)] {
        let expires = time::format_epoch_ms(CLOCK.checked_add_signed(delta).unwrap());
        store
            .write(move |db| {
                db.execute("UPDATE commands SET expires_at=?1", [&expires])
                    .map_err(OfficeError::storage)?;
                Ok(((), None))
            })
            .unwrap();
        let before = image(&store);
        let result = get(&store, CLOCK, &id, None);
        if expired {
            assert_eq!(result.unwrap_err().code, ErrorCode::Expired);
        } else {
            assert!(result.is_ok());
        }
        assert_eq!(image(&store), before);
    }
    // A lower wall clock cannot reopen a window already closed by the high-water mark.
    store.run_sql_for_test("UPDATE meta SET value='1800000000002' WHERE key='clock_high_ms'");
    assert_eq!(
        get(&store, 0, &id, None).unwrap_err().code,
        ErrorCode::Expired
    );
    assert_eq!(time::format_epoch_ms(CLOCK), "2027-01-15T08:00:00.000Z");
    store.run_sql_for_test("UPDATE commands SET expires_at='2027-01-15T09:00:00.000+01:00'; UPDATE meta SET value='1800000000000' WHERE key='clock_high_ms'");
    assert_eq!(
        get(&store, CLOCK, &id, None).unwrap_err().code,
        ErrorCode::Expired
    );
}

#[test]
fn each_principal_field_matches_and_null_legacy_rows_match_every_caller() {
    let (_dir, store, command) = setup();
    let id = command.command_id().to_owned();
    submit_on(&store, command, CLOCK).unwrap();
    let different = [
        Principal {
            account: Some("A2"),
            ..principal()
        },
        Principal {
            member: Some("M2"),
            ..principal()
        },
        Principal {
            kind: "agent",
            ..principal()
        },
        Principal {
            account: None,
            ..principal()
        },
        Principal {
            member: None,
            ..principal()
        },
    ];
    for caller in different {
        assert!(get(&store, CLOCK, &id, Some(caller)).is_ok());
    }
    store.run_sql_for_test(
        "UPDATE commands SET principal_account='other',principal_member='other',actor_kind=NULL",
    );
    assert!(
        get(&store, CLOCK, &id, Some(principal())).is_ok(),
        "actor_kind NULL alone defines a legacy row"
    );
    store.run_sql_for_test("UPDATE commands SET principal_account='A1',principal_member='01ARZ3NDEKTSV4RRFFQ69G5FAV',actor_kind='human'");
    assert!(get(&store, CLOCK, &id, Some(principal())).is_ok());
    assert!(
        get(&store, CLOCK, &id, None).is_ok(),
        "legacy connections are the implicit owner until grants"
    );
    for caller in different {
        assert_eq!(
            get(&store, CLOCK, &id, Some(caller)).unwrap_err().code,
            ErrorCode::NotFound
        );
    }
    store.run_sql_for_test(
        "UPDATE commands SET principal_account=NULL,principal_member=NULL,actor_kind='agent'",
    );
    assert!(get(
        &store,
        CLOCK,
        &id,
        Some(Principal {
            account: None,
            member: None,
            kind: "agent"
        })
    )
    .is_ok());
    assert_eq!(
        get(&store, CLOCK, &id, Some(principal())).unwrap_err().code,
        ErrorCode::NotFound
    );
}

#[test]
fn mismatch_precedes_expiry_and_corrupt_answer_and_expiry_precedes_answer() {
    let (_dir, store, command) = setup();
    let id = command.command_id().to_owned();
    submit_on(&store, command, CLOCK).unwrap();
    store.run_sql_for_test("UPDATE commands SET principal_account='A1',principal_member='01ARZ3NDEKTSV4RRFFQ69G5FAV',actor_kind='human',expires_at='not a time',result='SECRET'");
    assert_eq!(
        get(
            &store,
            CLOCK,
            &id,
            Some(Principal {
                kind: "agent",
                ..principal()
            })
        )
        .unwrap_err()
        .code,
        ErrorCode::NotFound
    );
    assert!(matches!(
        store.read(|db| lookup(db, CLOCK, &id, Some(principal()))),
        Err(OfficeError::Invalid(
            "a stored command expiry is not readable"
        ))
    ));
    store.run_sql_for_test("UPDATE commands SET expires_at='1970-01-01T00:00:00.000Z'");
    assert_eq!(
        get(
            &store,
            CLOCK,
            &id,
            Some(Principal {
                kind: "agent",
                ..principal()
            })
        )
        .unwrap_err()
        .code,
        ErrorCode::NotFound
    );
    assert_eq!(
        get(&store, CLOCK, &id, Some(principal())).unwrap_err().code,
        ErrorCode::Expired
    );
    store.run_sql_for_test("UPDATE commands SET expires_at='9999-01-01T00:00:00.000Z'");
    assert!(matches!(
        store.read(|db| lookup(db, CLOCK, &id, None)),
        Err(OfficeError::Invalid(
            "a stored command answer is not readable"
        ))
    ));
}

#[test]
fn reuse_checks_principal_and_hash_without_changing_existing_expiry_replay() {
    let (_dir, store, command) = setup();
    let id = command.command_id().to_owned();
    let original = command.clone();
    let accepted = submit_on(&store, command, CLOCK).unwrap();
    store.run_sql_for_test("UPDATE commands SET principal_account='A1',principal_member='01ARZ3NDEKTSV4RRFFQ69G5FAV',actor_kind='human'");
    let before = image(&store);
    for caller in [
        principal(),
        Principal {
            account: Some("A2"),
            ..principal()
        },
        Principal {
            member: Some("M2"),
            ..principal()
        },
        Principal {
            kind: "agent",
            ..principal()
        },
    ] {
        let command = original.clone();
        let result = store
            .write(move |db| submit_as(db, CLOCK, &command, Some(caller)).map(|x| (x, None)))
            .unwrap();
        if caller.kind == "human" && caller.account == Some("A1") && caller.member == Some(OWNER) {
            assert_eq!(result.unwrap(), accepted);
        } else {
            assert_eq!(result.unwrap_err().code, ErrorCode::OperationReused);
        }
        assert_eq!(image(&store), before);
    }
    assert_eq!(
        submit_on(&store, original, CLOCK + 31 * DAY_MS).unwrap(),
        accepted,
        "lookup-first replay semantics stay unchanged"
    );
    let changed = OrgCommand::TeamCreate(TeamCreateParams {
        command_id: id,
        team_id: "T1".into(),
        name: "Different".into(),
        owner_team_id: None,
    });
    assert_eq!(
        submit_on(&store, changed.clone(), CLOCK).unwrap_err().code,
        ErrorCode::OperationReused
    );
    store.run_sql_for_test(
        "UPDATE commands SET principal_account=NULL,principal_member=NULL,actor_kind=NULL",
    );
    assert_eq!(
        submit_on(&store, changed, CLOCK).unwrap_err().code,
        ErrorCode::OperationReused
    );
}

#[test]
fn clock_corruption_and_missing_answers_fail_without_echoing_stored_content() {
    let (_dir, store, command) = setup();
    let id = command.command_id().to_owned();
    submit_on(&store, command, CLOCK).unwrap();
    store.run_sql_for_test("UPDATE meta SET value='SECRET' WHERE key='clock_high_ms'");
    assert!(matches!(
        store.read(|db| lookup(db, CLOCK, &id, None)),
        Err(OfficeError::Invalid(
            "the clock high-water mark is not readable"
        ))
    ));
    store.run_sql_for_test("DELETE FROM meta WHERE key='clock_high_ms'");
    assert!(get(&store, CLOCK, &id, None).is_ok());
    assert_eq!(
        image(&store).1,
        None,
        "a lookup must not write a clock mark"
    );
    store.run_sql_for_test("UPDATE commands SET result=NULL");
    assert!(matches!(
        store.read(|db| lookup(db, CLOCK, &id, None)),
        Err(OfficeError::Invalid(
            "a stored command answer is not readable"
        ))
    ));
    store.run_sql_for_test("DROP TABLE meta");
    assert!(matches!(
        store.read(|db| lookup(db, CLOCK, &id, None)),
        Err(OfficeError::Storage(_))
    ));
    store.run_sql_for_test("DROP TABLE commands");
    assert!(matches!(
        store.read(|db| lookup(db, CLOCK, &id, None)),
        Err(OfficeError::Storage(_))
    ));
}
