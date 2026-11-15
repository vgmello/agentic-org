use super::*;
use org_proto::inbox::{InboxChangedData, InboxList, InboxListParams, InboxState};
use org_proto::msg::{MsgBind, MsgBindParams, MsgSend, MsgSendParams, KIND_MSG};
use std::time::Instant;

fn owner(world: &World) -> Client {
    let client = Client::connect(socket(world)).unwrap();
    client
        .call::<MsgBind>(&MsgBindParams {
            member: Some("owner".into()),
            app_token: Some(token()),
        })
        .unwrap();
    client
}

fn open() -> InboxListParams {
    InboxListParams {
        state: InboxState::Open,
        kinds: None,
        project: None,
        team: None,
        cursor: None,
        limit: None,
    }
}

fn hint_before(sub: &org_client::Subscription, deadline: Instant) -> InboxChangedData {
    loop {
        let event = sub
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .expect("committed change did not deliver inbox.changed before the deadline");
        if event.event == "inbox.changed" {
            return serde_json::from_value(event.data).unwrap();
        }
    }
}

#[test]
fn inbox_hint_real_mailbox_commit_advances_once_and_filters_the_same_hint_per_delivery() {
    let _processes = process_isolation();
    let world = World::new();
    let _server = create(&world);
    let receiver = owner(&world);
    let sender = owner(&world);
    let nonowner = Client::connect(socket(&world)).unwrap();
    bind(&nonowner, token()).unwrap();
    let before = receiver.call::<InboxList>(&open()).unwrap();
    let owner_sub = receiver.subscribe().unwrap();
    let nonowner_sub = nonowner.subscribe().unwrap();
    let params = MsgSendParams {
        id: command_id(71),
        to: "owner".into(),
        kind: KIND_MSG.into(),
        priority: None,
        subject: Some("Inbox hook probe".into()),
        body: "A message changes no currently registered inbox count".into(),
        meta: Default::default(),
        reply_to: None,
    };
    let deadline = Instant::now() + Duration::from_secs(1);
    sender.call::<MsgSend>(&params).unwrap();
    let hint = hint_before(&owner_sub, deadline);
    assert_eq!(hint.counts, before.counts);
    assert_eq!(hint.as_of.epoch, before.as_of.epoch);
    assert_eq!(hint.as_of.seq, before.as_of.seq + 1);
    assert!(hint.degraded.is_empty());
    assert!(receiver.hello().unwrap().supports("inbox.changed"));
    while let Ok(event) =
        nonowner_sub.recv_timeout(deadline.saturating_duration_since(Instant::now()))
    {
        assert_ne!(
            event.event, "inbox.changed",
            "a non-owner received the hint"
        );
    }
    // Replaying the same send commits nothing and must neither recount nor emit.
    sender.call::<MsgSend>(&params).unwrap();
    let replay_deadline = Instant::now() + Duration::from_millis(350);
    while let Ok(event) =
        owner_sub.recv_timeout(replay_deadline.saturating_duration_since(Instant::now()))
    {
        assert_ne!(
            event.event, "inbox.changed",
            "a replay emitted a second hint"
        );
    }
    assert_eq!(
        receiver.call::<InboxList>(&open()).unwrap().as_of,
        hint.as_of
    );
}

/// Build the org-owned fixture before any worker starts; SQL stays inside org-office.
fn unplaced(world: &World) -> String {
    use org_proto::org::{AgentProfile, AgentRuntime, AgentStatus};
    let office = org_office::Office::create(
        world.folder(OFFICE),
        &org_office::NewOffice {
            office_id: OFFICE.into(),
            office_name: "HQ".into(),
            role: org_office::OfficeRole::Hq,
            org_id: ORG.into(),
            org_name: "Acme".into(),
        },
        org_office::Key::from_db_key(db_key()),
    )
    .unwrap();
    let snapshot = office.org_snapshot().unwrap();
    let owner = snapshot
        .members
        .into_iter()
        .find(|member| member.owner)
        .unwrap();
    let mut agent = owner.clone();
    agent.id = command_id(72);
    agent.name = "Unplaced agent".into();
    agent.kind = org_proto::MemberKind::Agent;
    agent.owner = false;
    agent.office_id = None;
    agent.agent = Some(AgentProfile {
        role: "engineer".into(),
        title: "Engineer".into(),
        focus: None,
        runtime: AgentRuntime::ClaudeCode,
        model: None,
        run_mode: None,
        schedule: None,
        home: None,
        messaging_name: "unplaced-probe".into(),
        status: AgentStatus::Active,
        offboarded_at: None,
    });
    let id = agent.id.clone();
    office.seed_organization_for_test(&[owner, agent], snapshot.position);
    drop(office);
    id
}

#[test]
fn inbox_hint_real_second_owner_offboard_clears_the_seeded_problem_within_one_second() {
    let _processes = process_isolation();
    let world = World::new();
    let id = unplaced(&world);
    let mut server = world.spawn(&["--office", OFFICE]);
    server.send(&start_message());
    assert!(matches!(server.event(), ServerEvent::Ready { .. }));
    let receiver = owner(&world);
    let actor = owner(&world);
    let before = receiver.call::<InboxList>(&open()).unwrap();
    assert_eq!((before.counts.problems, before.counts.total), (1, 1));
    let sub = receiver.subscribe().unwrap();
    let deadline = Instant::now() + Duration::from_secs(1);
    actor
        .call::<org_proto::org::MemberOffboard>(&org_proto::org::MemberOffboardParams {
            command_id: command_id(73),
            member_id: id,
            expected_version: 1,
        })
        .unwrap();
    let hint = hint_before(&sub, deadline);
    assert_eq!((hint.counts.problems, hint.counts.total), (0, 0));
    assert_eq!(hint.as_of.epoch, before.as_of.epoch);
    assert_eq!(hint.as_of.seq, before.as_of.seq + 1);
    assert!(hint.degraded.is_empty());
    let after = receiver.call::<InboxList>(&open()).unwrap();
    assert_eq!(after.counts, hint.counts);
    assert!(after.items.is_empty());
}

fn local_office(world: &World, id: &str) -> org_office::Office {
    org_office::Office::create(
        world.folder(id),
        &org_office::NewOffice {
            office_id: id.into(),
            office_name: id.into(),
            role: org_office::OfficeRole::Hq,
            org_id: ORG.into(),
            org_name: "Acme".into(),
        },
        org_office::Key::from_db_key(db_key()),
    )
    .unwrap()
}

#[test]
fn inbox_hint_registration_uses_real_unbound_bound_restore_and_replica_authority() {
    use org_sync::organization::{OrgOutput, OrgStore};
    let _processes = process_isolation();
    for state in ["unbound", "hq", "restore", "satellite"] {
        let world = World::new();
        let office = local_office(&world, OFFICE);
        if state == "satellite" {
            let source = local_office(&world, "SOURCE");
            source
                .bind_replication_hq(Default::default(), 1024 * 1024)
                .unwrap();
            let pin = source.replication_checkpoint().unwrap();
            office
                .bind_replication_satellite(
                    pin.hq_office,
                    pin.ca,
                    pin.highest_epoch,
                    Default::default(),
                )
                .unwrap();
        } else if state != "unbound" {
            office
                .bind_replication_hq(Default::default(), 1024 * 1024)
                .unwrap();
            if state == "restore" {
                // Use the existing trusted store contract to seed the local journal fence.
                office
                    .transact(std::sync::Arc::new(|tx| {
                        let mut checkpoint = tx.checkpoint()?;
                        checkpoint.restore_pending = true;
                        tx.set_authority(&checkpoint)?;
                        Ok(OrgOutput::AuthorityAccepted {
                            snapshot_required: false,
                        })
                    }))
                    .unwrap();
            }
        }
        drop(office);
        let mut run = world.spawn(&["--office", OFFICE]);
        run.send(&start_message());
        assert!(matches!(run.event(), ServerEvent::Ready { .. }), "{state}");
        // A raw synchronous hello owns no detached client helpers across the next spawn.
        let mut reader = BufReader::new(UnixStream::connect(socket(&world)).unwrap());
        frame::write_message(
            reader.get_mut(),
            &serde_json::json!({"id":1,"method":"hello","params":{}}),
        )
        .unwrap();
        let reply: org_proto::Response = frame::read_message(&mut reader).unwrap().unwrap();
        let hello = reply.into_result::<org_proto::Hello>().unwrap();
        assert_eq!(
            hello.supports("inbox.list"),
            state != "satellite",
            "{state}"
        );
        assert_eq!(
            hello.supports("inbox.changed"),
            state != "satellite",
            "{state}"
        );
        drop(reader);
        assert!(run.host_goes_away().success());
    }
}
