//! The owner's organization commands through the real worker, started by the host. Persons and the
//! owner act only through grants on host channels: each case opens a local host channel over the
//! transfer socket and signs a grant for the owner with the test grant key. Agents bind on the
//! office socket by handle, with no token.
use super::*;
use org_office_launch::{ChannelKind, TransferRecord};
use org_office_server::test_support::{signed_grant, ChannelCaller};
use org_proto::{ApiError, Call};

/// A worker started by the host on the office the worker created.
pub(super) struct Live {
    pub(super) session: Session,
    channels: u32,
}

/// Starts the stored office through the launch contract and waits for `ready`.
pub(super) fn live(world: &World) -> Live {
    let mut session = handshake(
        world,
        FirstLine::Start,
        INSTANCE,
        Some(office_secrets_key()),
    );
    let ready = session.event();
    assert_eq!(ready["event"], "ready", "{ready}");
    Live {
        session,
        channels: 0,
    }
}

impl Live {
    /// A new local host channel, said hello to at api 2, acting as `member` when one is named.
    pub(super) fn caller(&mut self, member: Option<&str>) -> Caller {
        self.channels += 1;
        let id = format!("channel-{}", self.channels);
        let (source, peer) = UnixStream::pair().unwrap();
        peer.set_read_timeout(Some(WAIT)).unwrap();
        let record = org_office_launch::encode_transfer_record(&TransferRecord::Channel {
            kind: ChannelKind::Local,
            office_id: OFFICE.into(),
            channel_id: id.clone(),
        })
        .unwrap();
        transfer::send_record(
            self.session.transfer.as_ref().unwrap(),
            &record,
            &[source.as_fd()],
        );
        // Channels this case dropped earlier are reported closed first, in order.
        let received = loop {
            let event = self.session.event();
            if event["event"] != "channel_closed" {
                break event;
            }
        };
        assert_eq!(received, json!({"v":1,"event":"fds_received","of":id}));
        drop(source);
        let mut caller = ChannelCaller::new(peer);
        assert_eq!(caller.hello()["result"]["api_version"], 2);
        if let Some(member) = member {
            caller = caller.with_grant(signed_grant(member, ORG, OFFICE, &id, "test-session"));
        }
        Caller(caller)
    }

    /// Ends the rendezvous: the worker closes its office and exits clean.
    pub(super) fn stop(self) {
        self.session.stop();
    }
}

/// A host channel with typed calls.
pub(super) struct Caller(ChannelCaller);

impl Caller {
    pub(super) fn call<C: Call>(&mut self, params: &C::Params) -> Result<C::Result, ApiError> {
        let answer = self
            .0
            .call(C::METHOD, serde_json::to_value(params).unwrap());
        match answer.get("result") {
            Some(result) => Ok(serde_json::from_value(result.clone())
                .unwrap_or_else(|e| panic!("{}: {e}: {answer}", C::METHOD))),
            None => Err(serde_json::from_value(answer["error"].clone())
                .unwrap_or_else(|e| panic!("{}: {e}: {answer}", C::METHOD))),
        }
    }

    /// The raw answer to `method`, for a method with no typed `Call`.
    pub(super) fn raw(&mut self, method: &str, params: serde_json::Value) -> serde_json::Value {
        self.0.call(method, params)
    }
}

/// Creates the office through the worker and returns its owner's member id.
fn seeded(world: &World) -> String {
    seed(world);
    OWNER.into()
}

/// The HR lead: the agent the creation made in the top team.
fn hr() -> String {
    plan().hr_employee_id
}

fn snapshot(caller: &mut Caller) -> org_proto::org::OrgSnapshot {
    caller
        .call::<org_proto::OrgSnapshotCall>(&org_proto::Empty {})
        .unwrap()
}

/// `team.create` of the team `TEAM` under the top team.
fn team(
    caller: &mut Caller,
    command: &str,
    name: &str,
) -> Result<org_proto::org::CommandAccepted, ApiError> {
    caller.call::<org_proto::org::TeamCreate>(&org_proto::org::TeamCreateParams {
        command_id: command.into(),
        team_id: TEAM.into(),
        name: name.into(),
        owner_team_id: Some(TOP_TEAM.into()),
    })
}

fn platform(snapshot: &org_proto::org::OrgSnapshot) -> Option<&org_proto::org::Team> {
    snapshot.teams.iter().find(|team| team.id == TEAM)
}

#[test]
fn the_snapshot_has_the_creation_at_position_1_1_and_survives_a_restart() {
    let _process = process_isolation();
    let world = World::new();
    let owner_id = seeded(&world);
    let mut first = live(&world);
    // The organization is not read by an unbound office-socket connection, nor by a host
    // channel without a grant.
    let client = Client::connect(socket(&world)).unwrap();
    assert_eq!(
        api_code(
            client
                .call::<org_proto::OrgSnapshotCall>(&org_proto::Empty {})
                .unwrap_err()
        ),
        org_proto::ErrorCode::NotAuthorized
    );
    assert_eq!(
        first
            .caller(None)
            .call::<org_proto::OrgSnapshotCall>(&org_proto::Empty {})
            .unwrap_err()
            .code,
        org_proto::ErrorCode::NotAuthorized
    );
    let snapshot = snapshot(&mut first.caller(Some(&owner_id)));
    assert_eq!(
        snapshot.position,
        org_proto::org::Position { epoch: 1, seq: 1 }
    );
    // The creation: the owner and the HR lead in one top team named after the organization.
    let owner = snapshot.members.iter().find(|m| m.owner).unwrap();
    assert_eq!(owner.id, owner_id);
    assert!(snapshot.members.iter().any(|m| m.id == hr()));
    let [top] = &snapshot.teams[..] else {
        panic!("{:?}", snapshot.teams)
    };
    assert_eq!(
        (top.id.as_str(), top.owner_team_id.as_deref()),
        (TOP_TEAM, None)
    );
    drop(client);
    first.stop();

    let mut again = live(&world);
    let after = self::snapshot(&mut again.caller(Some(&owner_id)));
    assert_eq!(after, snapshot, "the organization changed across a restart");
    again.stop();
}

#[test]
fn inbox_real_hq_registration_is_owner_only_read_only_and_changes_epoch_on_restart() {
    let _processes = process_isolation();
    use org_proto::inbox::{InboxCounts, InboxList, InboxListParams, InboxState};
    let world = World::new();
    let owner_id = seeded(&world);
    let mut first = live(&world);
    let client = Client::connect(socket(&world)).unwrap();
    assert!(client.hello().unwrap().supports("inbox.list"));
    // B6 wires `subscribe` and live `inbox.changed` hints on HQ offices.
    assert!(client.hello().unwrap().supports("inbox.changed"));
    let params = InboxListParams {
        state: InboxState::Open,
        kinds: None,
        project: None,
        team: None,
        cursor: None,
        limit: None,
        has_file: None,
        has_image: None,
    };
    assert_eq!(
        api_code(client.call::<InboxList>(&params).unwrap_err()),
        org_proto::ErrorCode::NotAuthorized
    );
    assert_eq!(
        first
            .caller(None)
            .call::<InboxList>(&params)
            .unwrap_err()
            .code,
        org_proto::ErrorCode::NotAuthorized
    );
    let mut owner = first.caller(Some(&owner_id));
    let before = snapshot(&mut owner);
    let page = owner.call::<InboxList>(&params).unwrap();
    // Nothing waits on the owner: the HR lead is not an agent waiting to be placed (B2).
    assert_eq!(page.counts, InboxCounts::default());
    assert!(page.items.is_empty() && page.next_cursor.is_none() && page.degraded.is_empty());
    // The position is the organization's committed directory generation.
    assert_eq!(page.as_of.seq, before.position.seq);
    assert_eq!(page.as_of.epoch.len(), 32);
    assert_eq!(snapshot(&mut owner), before);
    drop((client, owner));
    first.stop();

    let mut again = live(&world);
    let mut owner = again.caller(Some(&owner_id));
    let later = owner.call::<InboxList>(&params).unwrap();
    assert_ne!(later.as_of.epoch, page.as_of.epoch);
    assert_eq!(later.as_of.seq, before.position.seq);
    assert_eq!(snapshot(&mut owner), before);
    drop(owner);
    again.stop();
}

#[test]
fn only_the_grant_bound_owner_changes_the_organization_and_what_it_changed_survives_a_restart() {
    let _process = process_isolation();
    let world = World::new();
    let owner_id = seeded(&world);
    let mut first = live(&world);
    let command = command_id(1);
    let client = Client::connect(socket(&world)).unwrap();

    // Unbound on the office socket: refused, nothing changes. A garbled command is refused the
    // same way (not told apart from a good one).
    let refused = team_create(&client, &command, "Platform").unwrap_err();
    assert_eq!(api_code(refused), org_proto::ErrorCode::NotAuthorized);
    let garbled = client
        .call::<org_proto::org::TeamCreate>(&org_proto::org::TeamCreateParams {
            command_id: "not a ulid".into(),
            team_id: String::new(),
            name: String::new(),
            owner_team_id: None,
        })
        .unwrap_err();
    assert_eq!(api_code(garbled), org_proto::ErrorCode::NotAuthorized);
    // A host channel without a grant is refused too.
    assert_eq!(
        team(&mut first.caller(None), &command, "Platform")
            .unwrap_err()
            .code,
        org_proto::ErrorCode::NotAuthorized
    );
    let mut owner = first.caller(Some(&owner_id));
    assert_eq!(
        snapshot(&mut owner).position,
        org_proto::org::Position { epoch: 1, seq: 1 }
    );

    // With the owner's grant: the command commits, and a retry is the same answer.
    let accepted = team(&mut owner, &command, "Platform").unwrap();
    assert_eq!(
        accepted.position,
        Some(org_proto::org::Position { epoch: 1, seq: 2 })
    );
    assert_eq!(team(&mut owner, &command, "Platform").unwrap(), accepted);
    assert_eq!(
        team(&mut owner, &command, "Other").unwrap_err().code,
        org_proto::ErrorCode::OperationReused,
        "the same id with other params"
    );
    // Top-level teams commit with `owner_team_id: None`.
    let top_level = owner
        .call::<org_proto::org::TeamCreate>(&org_proto::org::TeamCreateParams {
            command_id: command_id(3),
            team_id: "019a8928-1337-7456-8456-0123456789b2".into(),
            name: "Elsewhere".into(),
            owner_team_id: None,
        })
        .unwrap();
    assert_eq!(top_level.status, org_proto::org::CommandStatus::Committed);
    // Operation lookup is backed by the store's receipts: returns committed/rejected for the
    // acting owner and refuses another actor or anonymous caller with `not_found`.
    let get = |id: &str| org_proto::OperationGetParams {
        kind: org_proto::OperationKind::Org,
        id: id.into(),
    };
    assert_eq!(
        owner
            .call::<org_proto::OperationGet>(&get(&command))
            .unwrap(),
        org_proto::OperationOutcome::Committed {
            result: serde_json::to_value(accepted).unwrap(),
        }
    );
    let reader = Client::connect(socket(&world)).unwrap();
    let lookup = |client: &Client, id: &str| client.call::<org_proto::OperationGet>(&get(id));
    assert_eq!(
        api_code(lookup(&reader, &command).unwrap_err()),
        org_proto::ErrorCode::NotFound
    );
    assert_eq!(
        api_code(lookup(&reader, "bad").unwrap_err()),
        org_proto::ErrorCode::InvalidRequest
    );
    let refusal_id = command_id(2);
    let refusal = team(&mut owner, &refusal_id, " ").unwrap_err();
    assert_eq!(
        owner
            .call::<org_proto::OperationGet>(&get(&refusal_id))
            .unwrap(),
        org_proto::OperationOutcome::Rejected { refusal }
    );
    let after = snapshot(&mut owner);
    assert_eq!(platform(&after).unwrap().name, "Platform");
    assert_eq!(after.teams.len(), 3);
    drop((client, reader, owner));
    first.stop();

    // After a restart the team is there, and both the old command id and operation.get answer.
    let mut again = live(&world);
    let mut owner = again.caller(Some(&owner_id));
    assert_eq!(
        owner
            .call::<org_proto::OperationGet>(&get(&command))
            .unwrap(),
        org_proto::OperationOutcome::Committed {
            result: serde_json::to_value(accepted).unwrap(),
        }
    );
    assert_eq!(snapshot(&mut owner), after);
    assert_eq!(team(&mut owner, &command, "Platform").unwrap(), accepted);
    assert_eq!(snapshot(&mut owner), after);
    drop(owner);
    again.stop();
}

/// Done-when 2: a `team.create` answered on a grant channel survives killing the worker right
/// after the answer (SIGKILL: no orderly stop), and the restart serves the team.
#[test]
fn a_committed_team_survives_killing_the_worker_after_its_answer() {
    let _process = process_isolation();
    let world = World::new();
    let owner_id = seeded(&world);
    let mut first = live(&world);
    let mut owner = first.caller(Some(&owner_id));
    let command = command_id(1);
    let accepted = team(&mut owner, &command, "Platform").unwrap();
    assert_eq!(accepted.status, org_proto::org::CommandStatus::Committed);
    let confirmed = world.writer.head().unwrap();
    first.session.run.child.kill().unwrap();
    assert!(!first.session.run.wait().success());
    drop((owner, first));

    let mut again = live(&world);
    let mut owner = again.caller(Some(&owner_id));
    let after = snapshot(&mut owner);
    assert_eq!(platform(&after).unwrap().name, "Platform");
    assert_eq!(team(&mut owner, &command, "Platform").unwrap(), accepted);
    assert!(world.writer.head().unwrap().ordinal >= confirmed.ordinal);
    drop(owner);
    again.stop();
    // The orderly stop after the restart leaves a clean-stop marker (the in-process test checks
    // that a drop without `close` leaves none).
    assert!(world.office().stopped_cleanly());
}

/// Done-when 1, through the real worker: the projection deleted after a team and an orderly
/// stop, the restart rebuilds it from the files and serves the same organization; the same
/// command id gets its original answer.
#[test]
fn deleting_the_runtime_folder_rebuilds_the_same_organization_on_restart() {
    let _process = process_isolation();
    let world = World::new();
    let owner_id = seeded(&world);
    let mut first = live(&world);
    let mut owner = first.caller(Some(&owner_id));
    let command = command_id(1);
    let accepted = team(&mut owner, &command, "Platform").unwrap();
    let before = snapshot(&mut owner);
    drop(owner);
    first.stop();
    let projection = world.data.join("runtime").join(OFFICE);
    assert!(projection.join("office.db").is_file());
    std::fs::remove_dir_all(&projection).unwrap();

    let mut again = live(&world);
    let mut owner = again.caller(Some(&owner_id));
    assert_eq!(snapshot(&mut owner), before);
    assert_eq!(team(&mut owner, &command, "Platform").unwrap(), accepted);
    drop(owner);
    again.stop();
    assert!(projection.join("office.db").is_file());
}

#[test]
fn who_lists_the_directory_with_cards_for_the_owner_and_not_for_an_unbound_connection() {
    let _process = process_isolation();
    use org_proto::directory::{MemberPresence, Who, WhoParams};
    use org_proto::org::MemberKind;
    let world = World::new();
    let owner_id = seeded(&world);
    let mut live = live(&world);
    let client = Client::connect(socket(&world)).unwrap();

    // Unbound: not_bound, whatever the params.
    let unbound = client.call::<Who>(&WhoParams::default()).unwrap_err();
    assert_eq!(api_code(unbound), org_proto::ErrorCode::NotBound);
    let mut owner = live.caller(Some(&owner_id));

    // The owner, whose handle is `owner` (offline until connected), and the HR lead
    // (ADR-0028 AVAIL-4: active idle agents report `Available`, no status line).
    let everyone = owner.call::<Who>(&WhoParams::default()).unwrap();
    let card = everyone.members.iter().find(|c| c.owner).unwrap();
    assert_eq!(card.id, owner_id);
    assert_eq!(card.handle.as_deref(), Some("owner"));
    let lead = everyone.members.iter().find(|c| c.id == hr()).unwrap();
    assert_eq!(lead.kind, MemberKind::Agent);
    assert_eq!(lead.presence, MemberPresence::Available);
    assert_eq!(lead.status, None);
    // Filters: by team name, by kind, by member.
    team(&mut owner, &command_id(1), "Platform").unwrap();
    let in_team = owner
        .call::<Who>(&WhoParams {
            team: Some("platform".into()),
            ..WhoParams::default()
        })
        .unwrap();
    assert!(in_team.members.is_empty(), "{:?}", in_team.members);
    let people = owner
        .call::<Who>(&WhoParams {
            kind: Some(MemberKind::Person),
            ..WhoParams::default()
        })
        .unwrap();
    assert!(people.members.iter().all(|c| c.kind == MemberKind::Person));
    assert!(people.members.iter().any(|c| c.id == owner_id));
    let by_id = owner
        .call::<Who>(&WhoParams {
            member: Some(hr()),
            ..WhoParams::default()
        })
        .unwrap();
    assert_eq!(by_id.members.len(), 1);
    let nobody = owner
        .call::<Who>(&WhoParams {
            member: Some("nobody".into()),
            ..WhoParams::default()
        })
        .unwrap_err();
    assert_eq!(nobody.code, org_proto::ErrorCode::NotFound);
    drop((client, owner));
    live.stop();
}

/// Messaging, status lines, `subscribe`, operation lookup and every organization command but
/// `team.create` lived only in the old tables. Until their families are rebuilt on the office
/// store (messaging and status B6, the rest B2) the real worker answers each of them
/// `unsupported` to the owner, while the directory and `team.create` keep working. The retired
/// phase-1 work writes have no host policy row: the owner's channel refuses them outright.
#[test]
fn calls_only_the_old_tables_served_answer_unsupported_through_the_real_worker() {
    let _process = process_isolation();
    use org_proto::directory::{Who, WhoParams};
    use org_proto::msg::{
        MsgInbox, MsgInboxParams, MsgRead, MsgReadParams, MsgSend, MsgSendParams, MsgWait,
        MsgWaitParams, KIND_MSG,
    };
    use org_proto::org::{TeamRename, TeamRenameParams};
    use org_proto::{ErrorCode, OperationGet, OperationGetParams, OperationKind};
    let world = World::new();
    let owner_id = seeded(&world);
    let mut live = live(&world);
    let mut owner = live.caller(Some(&owner_id));
    assert_eq!(
        team(&mut owner, &command_id(1), "Platform").unwrap().status,
        org_proto::org::CommandStatus::Committed
    );

    // No agent of this office can bind until agents are hired through HR (B2): the
    // creation's HR lead has no office yet. The agent's side of these calls is covered in
    // process (`serve.rs` tests); here the office socket is unbound.
    let refused = Client::connect(socket(&world)).unwrap();
    assert_eq!(
        api_code(
            refused
                .call::<MsgBind>(&MsgBindParams::member(hr()))
                .unwrap_err()
        ),
        ErrorCode::NotAuthorized
    );
    drop(refused);

    let message = |n: u8, to: &str| MsgSendParams {
        id: command_id(n),
        to: to.into(),
        kind: KIND_MSG.into(),
        priority: None,
        subject: None,
        body: "hello".into(),
        meta: Default::default(),
        reply_to: None,
        attachment_ids: Vec::new(),
    };
    let inbox = MsgInboxParams {
        include_all: true,
        from: None,
        thread_id: None,
        cursor: None,
        limit: None,
    };
    let wait = MsgWaitParams {
        timeout_ms: 30_000,
        from: None,
        urgent_only: false,
    };

    // `subscribe` on the office socket is served by B6 and refuses unbound callers.
    let subscribe = org_proto::SubscribeParams {
        from_position: None,
        from_work_position: None,
    };
    let unbound = Client::connect(socket(&world)).unwrap();
    assert_eq!(
        api_code(
            unbound
                .call::<org_proto::Subscribe>(&subscribe)
                .unwrap_err()
        ),
        ErrorCode::NotAuthorized
    );

    // The owner, through its grant on a host channel: messaging is now served by B6.
    let owner_unsupported = |error: ApiError| error.code == ErrorCode::Unsupported;
    let sent = owner.call::<MsgSend>(&message(11, "owner")).unwrap();
    let listed = owner.call::<MsgInbox>(&inbox).unwrap();
    assert!(listed.headers.iter().any(|h| h.id == sent.id));
    let read = owner
        .call::<MsgRead>(&MsgReadParams {
            id: Some(sent.id.clone()),
        })
        .unwrap();
    assert_eq!(
        read.message.as_ref().map(|m| m.header.id.as_str()),
        Some(sent.id.as_str())
    );
    // A person never waits on the office: refused at once, not held for 30 s.
    let started = std::time::Instant::now();
    owner.call::<MsgWait>(&wait).unwrap_err();
    assert!(started.elapsed() < Duration::from_secs(10));
    // The retired phase-1 work writes fail closed on the owner's host channel.
    for (method, params) in [
        (
            "task.create",
            serde_json::json!({"change_id":command_id(3), "item_id":command_id(4),
                "project_id":command_id(6), "title":"A task"}),
        ),
        (
            "work.delete",
            serde_json::json!({"change_id":command_id(5), "item_id":command_id(4),
                "expected_version":1}),
        ),
    ] {
        let answer = owner.raw(method, params);
        assert_eq!(
            answer["error"]["code"], "not_authorized",
            "{method}: {answer}"
        );
        assert_eq!(
            answer["error"]["message"], "request refused on host channel",
            "{method}: {answer}"
        );
    }
    // operation.get is served for org receipts (unknown ID -> not_found).
    assert_eq!(
        owner
            .call::<OperationGet>(&OperationGetParams {
                kind: OperationKind::Org,
                id: command_id(99),
            })
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
    // Project commands are wired on org-store (B3): unknown project_id -> not_found.
    assert_eq!(
        owner
            .call::<org_proto::org::ProjectAddTeam>(&org_proto::org::ProjectAddTeamParams {
                command_id: command_id(7),
                project_team_id: command_id(12),
                project_id: command_id(13),
                team_id: TEAM.into(),
            })
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
    // An unbuilt organization command (direct agent add is replaced by HR hire in C2).
    let add_agent: org_proto::org::MemberAddAgentParams =
        serde_json::from_value(serde_json::json!({
            "command_id": command_id(14),
            "member_id": command_id(15),
            "name": "Bot",
            "office_id": "0190a5d2-7c3e-7000-8000-000000000002",
            "profile": {
                "role": "engineer",
                "title": "Engineer",
                "runtime": "codex",
                "messaging_name": "bot",
                "status": "active"
            }
        }))
        .unwrap();
    assert!(owner_unsupported(
        owner
            .call::<org_proto::org::MemberAddAgent>(&add_agent)
            .unwrap_err()
    ));
    // A wired organization command commits through the real worker.
    let renamed = owner
        .call::<TeamRename>(&TeamRenameParams {
            command_id: command_id(8),
            team_id: TEAM.into(),
            name: "Renamed".into(),
            expected_version: 1,
        })
        .unwrap();
    assert_eq!(renamed.status, org_proto::org::CommandStatus::Committed);

    // The directory read still answers: no status line, active idle HR lead is Available (AVAIL-4).
    let who = owner
        .call::<Who>(&WhoParams {
            member: Some(hr()),
            ..WhoParams::default()
        })
        .unwrap();
    let [lead] = &who.members[..] else {
        panic!("{:?}", who.members)
    };
    assert_eq!(lead.status, None);
    assert_eq!(
        lead.presence,
        org_proto::directory::MemberPresence::Available
    );
    drop((unbound, owner));
    live.stop();
}

/// A store that stops (here: the host refuses the confirmation of a flushed write) is fatal
/// for the worker: the write is not answered as committed, the worker logs the store's fixed
/// text and exits 1, so the host restarts it from what is on the drive.
#[test]
fn a_store_that_stops_exits_the_worker_1() {
    let _process = process_isolation();
    let world = World::new();
    let owner_id = seeded(&world);
    let mut first = live(&world);
    let mut owner = first.caller(Some(&owner_id));
    let held = world.writer.head().unwrap();
    // The host now holds a head this office never wrote: it refuses the next confirmation.
    let mut elsewhere = held.clone();
    elsewhere.ordinal += 100;
    world.writer.set_head(Some(elsewhere));
    let refused = team(&mut owner, &command_id(1), "Platform");
    assert!(
        !matches!(&refused, Ok(accepted) if accepted.status == org_proto::org::CommandStatus::Committed),
        "{refused:?}"
    );
    drop(owner);
    let mut session = first.session;
    assert_eq!(session.run.wait().code(), Some(1));
    let log = session.run.stderr();
    assert!(
        log.contains("agentic-org-office: the office store stopped"),
        "{log}"
    );
    session.no_stdout();
    drop(session);
    // With its custody back, the office starts again from the drive.
    world.writer.set_head(Some(held));
    let mut again = live(&world);
    let snapshot = snapshot(&mut again.caller(Some(&owner_id)));
    assert!(snapshot.members.iter().any(|m| m.id == owner_id));
    again.stop();
}
