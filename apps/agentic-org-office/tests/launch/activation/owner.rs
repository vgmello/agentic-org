//! The owner's organization commands through the real worker, started by the host. Persons and the
//! owner act only through grants on host channels: each case opens a local host channel over the
//! transfer socket and signs a grant for the owner with the test grant key. Agents bind on the
//! office socket by handle, with no token.
use super::*;
use agentic_org_office::test_support::{signed_grant, ChannelCaller};
use org_office_launch::{ChannelKind, TransferRecord};
use org_proto::org::{
    AgentProfile, AgentRuntime, AgentStatus, MemberAddAgent, MemberAddAgentParams,
};
use org_proto::{ApiError, Call};

/// A worker started by the host (it adopts the library-stored office on its first start).
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
            &[source.as_raw_fd()],
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
}

/// Stores the office with the library and returns its owner's member id.
fn seeded(world: &World) -> String {
    seed(world);
    let office = org_office::Office::open(
        world.folder(OFFICE),
        org_office::Key::from_office_secrets_key(office_secrets_key()),
    )
    .unwrap();
    let owner = office
        .org_snapshot()
        .unwrap()
        .members
        .into_iter()
        .find(|member| member.owner)
        .expect("a stored office has its owner")
        .id;
    drop(office);
    owner
}

/// Hiring `member_id` as the agent `handle` into this office.
pub(super) fn hire(n: u8, member_id: &str, name: &str, handle: &str) -> MemberAddAgentParams {
    MemberAddAgentParams {
        command_id: command_id(n),
        member_id: member_id.into(),
        name: name.into(),
        office_id: OFFICE.into(),
        profile: AgentProfile {
            role: "engineer".into(),
            title: "Engineer".into(),
            focus: None,
            runtime: AgentRuntime::ClaudeCode,
            model: None,
            run_mode: None,
            schedule: None,
            home: None,
            messaging_name: handle.into(),
            status: AgentStatus::Active,
            offboarded_at: None,
        },
    }
}

fn snapshot(caller: &mut Caller) -> org_proto::org::OrgSnapshot {
    caller
        .call::<org_proto::OrgSnapshotCall>(&org_proto::Empty {})
        .unwrap()
}

fn team(
    caller: &mut Caller,
    command: &str,
    name: &str,
) -> Result<org_proto::org::CommandAccepted, ApiError> {
    caller.call::<org_proto::org::TeamCreate>(&org_proto::org::TeamCreateParams {
        command_id: command.into(),
        team_id: "T1".into(),
        name: name.into(),
        owner_team_id: None,
    })
}

/// An initial replica has no organization yet, so it has no owner a grant could name: the real
/// worker refuses the owner's grant as it refuses an unbound connection. (A bound caller's
/// snapshot answering `unavailable` is covered in process, `serve.rs` tests.)
#[test]
fn initial_replica_refuses_the_snapshot_to_an_unbound_connection_and_to_an_owner_grant() {
    let _process = process_isolation();
    let world = World::new();
    let owner = seeded(&world);
    let source = org_office::Office::create(
        world.folder("SOURCE"),
        &org_office::NewOffice {
            office_id: "SOURCE".into(),
            office_name: "Source".into(),
            role: org_office::OfficeRole::Hq,
            org_id: ORG.into(),
            org_name: "Acme".into(),
        },
        org_office::Key::from_office_secrets_key(office_secrets_key()),
    )
    .unwrap();
    source
        .bind_replication_hq(Default::default(), 1024 * 1024)
        .unwrap();
    let pin = source.replication_checkpoint().unwrap();
    drop(source);
    let office = org_office::Office::open(
        world.folder(OFFICE),
        org_office::Key::from_office_secrets_key(office_secrets_key()),
    )
    .unwrap();
    office
        .bind_replication_satellite(pin.hq_office, pin.ca, pin.highest_epoch, Default::default())
        .unwrap();
    drop(office);

    let mut live = live(&world);
    let client = Client::connect(socket(&world)).unwrap();
    assert_eq!(
        api_code(
            client
                .call::<org_proto::OrgSnapshotCall>(&org_proto::Empty {})
                .unwrap_err()
        ),
        org_proto::ErrorCode::NotAuthorized
    );
    let mut owner = live.caller(Some(&owner));
    assert_eq!(
        owner
            .call::<org_proto::OrgSnapshotCall>(&org_proto::Empty {})
            .unwrap_err()
            .code,
        org_proto::ErrorCode::NotAuthorized
    );
    drop((client, owner));
    live.stop();
}

#[test]
fn the_snapshot_has_the_owner_at_position_1_1_and_survives_a_restart() {
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
    let [owner] = &snapshot.members[..] else {
        panic!("{:?}", snapshot.members)
    };
    assert!(owner.owner && org_proto::is_valid_member_id(&owner.id));
    assert_eq!(owner.id, owner_id);
    assert!(snapshot.teams.is_empty() && snapshot.memberships.is_empty());
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
    // Hints travel only on `subscribe`, which this office does not serve yet (B6).
    assert!(!client.hello().unwrap().supports("inbox.changed"));
    let params = InboxListParams {
        state: InboxState::Open,
        kinds: None,
        project: None,
        team: None,
        cursor: None,
        limit: None,
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
    assert_eq!(page.counts, InboxCounts::default());
    assert!(page.items.is_empty() && page.next_cursor.is_none() && page.degraded.is_empty());
    assert_eq!(page.as_of.seq, 0);
    assert_eq!(page.as_of.epoch.len(), 32);
    assert_eq!(snapshot(&mut owner), before);
    drop((client, owner));
    first.stop();

    let mut again = live(&world);
    let mut owner = again.caller(Some(&owner_id));
    let later = owner.call::<InboxList>(&params).unwrap();
    assert_ne!(later.as_of.epoch, page.as_of.epoch);
    assert_eq!(later.as_of.seq, 0);
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
    let get = |id: &str| org_proto::OperationGetParams {
        kind: org_proto::OperationKind::Org,
        id: id.into(),
    };
    // Operation lookup read the old tables: the owner's lookup is `unsupported` until B2 maps
    // the store's receipts. What is refused before the lookup is unchanged.
    let unsupported = |caller: &mut Caller, id: &str| {
        caller
            .call::<org_proto::OperationGet>(&get(id))
            .unwrap_err()
            .code
            == org_proto::ErrorCode::Unsupported
    };
    assert!(unsupported(&mut owner, &command));
    // Owner/NULL rows are private even to a legacy-first unbound connection.
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
    assert_eq!(
        api_code(lookup(&reader, &command_id(9)).unwrap_err()),
        org_proto::ErrorCode::NotFound
    );
    let refusal_id = command_id(2);
    team(&mut owner, &refusal_id, " ").unwrap_err();
    assert_eq!(
        api_code(lookup(&reader, &refusal_id).unwrap_err()),
        org_proto::ErrorCode::NotFound
    );
    reader.hello().unwrap();
    assert_eq!(
        api_code(lookup(&reader, &command).unwrap_err()),
        org_proto::ErrorCode::NotFound
    );
    let mut other = first.caller(Some(&owner_id));
    assert!(unsupported(&mut other, &refusal_id));
    assert!(unsupported(&mut other, &command));
    let after = snapshot(&mut owner);
    assert_eq!(after.teams.len(), 1);
    assert_eq!(after.teams[0].name, "Platform");
    drop((client, reader, owner, other));
    first.stop();

    // After a restart the team is there, and the old command id still gets its answer.
    let mut again = live(&world);
    let client = Client::connect(socket(&world)).unwrap();
    assert_eq!(
        api_code(lookup(&client, &command).unwrap_err()),
        org_proto::ErrorCode::NotFound
    );
    let mut owner = again.caller(Some(&owner_id));
    assert!(unsupported(&mut owner, &command));
    assert_eq!(snapshot(&mut owner), after);
    assert_eq!(team(&mut owner, &command, "Platform").unwrap(), accepted);
    assert_eq!(snapshot(&mut owner), after);
    drop((client, owner));
    again.stop();
}

#[test]
fn the_owner_hires_an_agent_into_the_office_and_it_survives_a_restart() {
    let _process = process_isolation();
    use org_proto::org::{MemberOffboard, MemberOffboardParams};
    let world = World::new();
    let owner_id = seeded(&world);
    let mut first = live(&world);
    let mut owner = first.caller(Some(&owner_id));
    let member_id = "AGENT00000000000000000000A";
    let elsewhere = |n: u8, office: &str| MemberAddAgentParams {
        office_id: office.into(),
        ..hire(n, member_id, "Pixel", "pixel")
    };

    // An office this organization does not have: not_found, nothing added.
    let nowhere = owner
        .call::<MemberAddAgent>(&elsewhere(1, "NOSUCHOFFICE"))
        .unwrap_err();
    assert_eq!(nowhere.code, org_proto::ErrorCode::NotFound);
    // Its own office: hired.
    let hired = owner
        .call::<MemberAddAgent>(&hire(2, member_id, "Pixel", "pixel"))
        .unwrap();
    assert_eq!(
        hired.position,
        Some(org_proto::org::Position { epoch: 1, seq: 2 })
    );
    // Its handle is taken, in any case.
    let again = owner
        .call::<MemberAddAgent>(&hire(3, member_id, "Pixel", "PIXEL"))
        .unwrap_err();
    assert_eq!(again.code, org_proto::ErrorCode::AlreadyExists);
    let agent = snapshot(&mut owner)
        .members
        .into_iter()
        .find(|m| m.id == member_id)
        .unwrap();
    assert_eq!(agent.office_id.as_deref(), Some(OFFICE));
    drop(owner);
    first.stop();

    // After a restart: still there; offboarded, and its handle stays taken.
    let mut later = live(&world);
    let mut owner = later.caller(Some(&owner_id));
    let offboarded = owner
        .call::<MemberOffboard>(&MemberOffboardParams {
            command_id: command_id(4),
            member_id: member_id.into(),
            expected_version: 1,
        })
        .unwrap();
    assert_eq!(
        offboarded.position,
        Some(org_proto::org::Position { epoch: 1, seq: 3 })
    );
    let clash = owner
        .call::<MemberAddAgent>(&hire(5, "AGENT00000000000000000000B", "Pixel", "pixel"))
        .unwrap_err();
    assert_eq!(clash.code, org_proto::ErrorCode::AlreadyExists);
    let agent = snapshot(&mut owner)
        .members
        .into_iter()
        .find(|m| m.id == member_id)
        .unwrap();
    let profile = agent.agent.as_ref().unwrap();
    assert_eq!(profile.status, AgentStatus::Offboarded);
    assert!(profile.offboarded_at.is_some());
    drop(owner);
    later.stop();
}

#[test]
fn who_lists_the_directory_with_cards_for_the_owner_and_not_for_an_unbound_connection() {
    let _process = process_isolation();
    use org_proto::directory::{Who, WhoParams};
    use org_proto::org::{
        MemberKind, MemberOffboard, MemberOffboardParams, TeamAddMember, TeamAddMemberParams,
    };
    let world = World::new();
    let owner_id = seeded(&world);
    let mut live = live(&world);
    let client = Client::connect(socket(&world)).unwrap();

    // Unbound: not_bound, whatever the params.
    let unbound = client.call::<Who>(&WhoParams::default()).unwrap_err();
    assert_eq!(api_code(unbound), org_proto::ErrorCode::NotBound);
    let mut owner = live.caller(Some(&owner_id));

    // The owner alone to begin with: their handle is `owner`, not their name.
    let first = owner.call::<Who>(&WhoParams::default()).unwrap();
    let [card] = &first.members[..] else {
        panic!("{:?}", first.members)
    };
    assert!(card.owner);
    assert_eq!(card.handle.as_deref(), Some("owner"));

    // A team, an agent in it (placed in this office), and what the card then says.
    team(&mut owner, &command_id(1), "Platform").unwrap();
    let agent_id = "AGENT00000000000000000000A";
    let mut add = hire(2, agent_id, "Pixel the engineer", "pixel");
    add.profile.title = "Senior Engineer".into();
    owner.call::<MemberAddAgent>(&add).unwrap();
    owner
        .call::<TeamAddMember>(&TeamAddMemberParams {
            command_id: command_id(3),
            membership_id: "m1".into(),
            team_id: "T1".into(),
            member_id: agent_id.into(),
            title_in_team: Some("Lead".into()),
        })
        .unwrap();

    let by_handle = owner
        .call::<Who>(&WhoParams {
            member: Some("PIXEL".into()),
            ..WhoParams::default()
        })
        .unwrap();
    let [pixel] = &by_handle.members[..] else {
        panic!("{:?}", by_handle.members)
    };
    assert_eq!(pixel.id, agent_id);
    assert_eq!(pixel.kind, MemberKind::Agent);
    assert_eq!(pixel.role_title.as_deref(), Some("Senior Engineer"));
    assert_eq!(pixel.office.as_ref().map(|o| o.name.as_str()), Some("HQ"));
    assert_eq!(pixel.teams[0].name, "Platform");
    assert_eq!(pixel.teams[0].title_in_team.as_deref(), Some("Lead"));
    // Filters: by team name, and by kind.
    let in_team = owner
        .call::<Who>(&WhoParams {
            team: Some("platform".into()),
            ..WhoParams::default()
        })
        .unwrap();
    assert_eq!(in_team.members.len(), 1);
    let people = owner
        .call::<Who>(&WhoParams {
            kind: Some(MemberKind::Person),
            ..WhoParams::default()
        })
        .unwrap();
    assert!(people.members.iter().all(|c| c.kind == MemberKind::Person));
    let nobody = owner
        .call::<Who>(&WhoParams {
            member: Some("nobody".into()),
            ..WhoParams::default()
        })
        .unwrap_err();
    assert_eq!(nobody.code, org_proto::ErrorCode::NotFound);

    // Offboarded: gone from the directory, and naming it is not_found.
    owner
        .call::<MemberOffboard>(&MemberOffboardParams {
            command_id: command_id(4),
            member_id: agent_id.into(),
            expected_version: 1,
        })
        .unwrap();
    let after = owner.call::<Who>(&WhoParams::default()).unwrap();
    assert!(after.members.iter().all(|c| c.id != agent_id));
    let gone = owner
        .call::<Who>(&WhoParams {
            member: Some("pixel".into()),
            ..WhoParams::default()
        })
        .unwrap_err();
    assert_eq!(gone.code, org_proto::ErrorCode::NotFound);
    drop((client, owner));
    live.stop();
}

/// Messaging, status lines, work, `subscribe` and operation lookup lived only in the old
/// tables. Until their families are rebuilt on the office store (work B4, messaging and status
/// B6, operation lookup B2) the real worker answers each of them `unsupported`, for an agent and
/// for the owner alike, while binding, the directory and organization commands keep working.
#[test]
fn calls_only_the_old_tables_served_answer_unsupported_through_the_real_worker() {
    let _process = process_isolation();
    use org_proto::directory::{StatusSet, StatusSetParams, Who, WhoParams};
    use org_proto::msg::{
        MsgInbox, MsgInboxParams, MsgRead, MsgReadParams, MsgSend, MsgSendParams, MsgWait,
        MsgWaitParams, KIND_MSG,
    };
    use org_proto::work::{TaskCreate, TaskCreateParams, WorkDelete, WorkDeleteParams};
    use org_proto::{ErrorCode, OperationGet, OperationGetParams, OperationKind};
    let world = World::new();
    let owner_id = seeded(&world);
    let mut live = live(&world);
    let mut owner = live.caller(Some(&owner_id));
    // Hiring is an organization command: it still commits on the office's organization data.
    let agent_id = "AGENT00000000000000000000A";
    let hired = owner
        .call::<MemberAddAgent>(&hire(1, agent_id, "Pixel", "pixel"))
        .unwrap();
    assert_eq!(hired.status, org_proto::org::CommandStatus::Committed);

    // The agent binds with no token, through the directory.
    let agent = Client::connect(socket(&world)).unwrap();
    let bound = agent
        .call::<MsgBind>(&MsgBindParams::member("pixel"))
        .unwrap();
    assert_eq!(bound.member_id.as_deref(), Some(agent_id));

    let message = |n: u8, to: &str| MsgSendParams {
        id: command_id(n),
        to: to.into(),
        kind: KIND_MSG.into(),
        priority: None,
        subject: None,
        body: "hello".into(),
        meta: Default::default(),
        reply_to: None,
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
    let task: TaskCreateParams = serde_json::from_value(serde_json::json!({
        "change_id":command_id(3), "item_id":command_id(4),
        "project_id":command_id(6), "title":"A task"
    }))
    .unwrap();
    let delete = WorkDeleteParams {
        change_id: command_id(5),
        item_id: task.item_id.clone(),
        expected_version: 1,
    };

    // The agent, on the office socket.
    let unsupported = |error: org_client::ClientError| api_code(error) == ErrorCode::Unsupported;
    assert!(unsupported(
        agent.call::<MsgSend>(&message(10, "owner")).unwrap_err()
    ));
    assert!(unsupported(agent.call::<MsgInbox>(&inbox).unwrap_err()));
    assert!(unsupported(
        agent
            .call::<MsgRead>(&MsgReadParams { id: None })
            .unwrap_err()
    ));
    // Answered at once, not held for 30 s.
    assert!(unsupported(agent.call::<MsgWait>(&wait).unwrap_err()));
    assert!(unsupported(
        agent
            .call::<StatusSet>(&StatusSetParams {
                text: Some("Wiring the broker".into()),
                ttl_ms: None,
            })
            .unwrap_err()
    ));
    assert!(unsupported(agent.call::<TaskCreate>(&task).unwrap_err()));
    // `subscribe` on the office socket, bound or not. (Host channels refuse it by their own
    // policy, before the office is asked.)
    let subscribe = org_proto::SubscribeParams {
        from_position: None,
        from_work_position: None,
    };
    assert!(unsupported(
        agent.call::<org_proto::Subscribe>(&subscribe).unwrap_err()
    ));
    let unbound = Client::connect(socket(&world)).unwrap();
    assert!(unsupported(
        unbound
            .call::<org_proto::Subscribe>(&subscribe)
            .unwrap_err()
    ));

    // The owner, through its grant on a host channel.
    let owner_unsupported = |error: ApiError| error.code == ErrorCode::Unsupported;
    assert!(owner_unsupported(
        owner.call::<MsgSend>(&message(11, "pixel")).unwrap_err()
    ));
    assert!(owner_unsupported(
        owner.call::<MsgInbox>(&inbox).unwrap_err()
    ));
    assert!(owner_unsupported(
        owner.call::<WorkDelete>(&delete).unwrap_err()
    ));
    assert!(owner_unsupported(
        owner
            .call::<OperationGet>(&OperationGetParams {
                kind: OperationKind::Org,
                id: command_id(1),
            })
            .unwrap_err()
    ));

    // The directory read still answers: no status line, everyone offline.
    let who = owner
        .call::<Who>(&WhoParams {
            member: Some("pixel".into()),
            ..WhoParams::default()
        })
        .unwrap();
    let [pixel] = &who.members[..] else {
        panic!("{:?}", who.members)
    };
    assert_eq!(pixel.id, agent_id);
    assert_eq!(pixel.status, None);
    assert_eq!(
        pixel.presence,
        org_proto::directory::MemberPresence::Offline
    );
    drop((agent, unbound, owner));
    live.stop();
}
