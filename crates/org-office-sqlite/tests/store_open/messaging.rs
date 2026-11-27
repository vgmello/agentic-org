//! Engine-independent B6 messaging conformance suite (`TASK-45` AC #1, `doc-90`, `WATCH-1/2/9`).
//!
//! Runs identically on both [`EngineChoice::Sqlite`] (backed by isolated `protected/messaging.db`)
//! and [`EngineChoice::Memory`].

use org_proto::chat::{
    ChatChannelCreateParams, ChatChannelMembershipParams, ChatChannelsParams,
    ChatConversationsParams, ChatInboxParams, ChatMarkReadParams, ChatMessagesParams,
    ChatMuteParams, ChatReactParams, ChatSendParams, ConversationKind, DeliveredBecause,
    DmOpenParams, MsgAcceptParams, MuteMode, ReactionKind,
};
use org_proto::msg::{MessageLifecycleState, Meta};
use org_proto::ErrorCode;

use super::*;

pub(super) fn seed_org_with_two_people(
    place: &Place,
    engine: EngineChoice,
) -> (OfficeStore, Id, Id) {
    let store = place.create(engine);
    let here = identity();
    let cmd = creation_command();
    let owner = cmd.creator.employee_id.clone();
    let top_team = cmd.team_id.clone();
    let create = CreateOrganizationOp {
        operation_id: id(1),
        here: here.clone(),
        command: cmd,
        ca: ca_material(&here.organization_id, 1),
    };
    assert_eq!(store.run(create).unwrap().outcome, Outcome::Completed);

    let ada = id(200);
    let add_ada = AddPersonOp {
        operation_id: id(2),
        here: here.clone(),
        owner: owner.clone(),
        by: Actor::Employee(owner.clone()),
        command: org_office::organization_family::features::AddPerson::new(
            ada.clone(),
            "ada",
            "Ada",
            top_team,
        ),
    };
    assert_eq!(store.run(add_ada).unwrap().outcome, Outcome::Completed);
    (store, owner, ada)
}

#[test]
fn engine_independent_messaging_conversations_channels_mentions_reactions_and_rebuild() {
    for engine in ENGINES {
        let place = Place::new();
        let (store, owner, ada) = seed_org_with_two_people(&place, engine);
        let here = identity();
        let now = "2026-11-21T12:00:00.000Z".to_owned();

        // 1. Auto-channels (#general and root team channel) appear without manual creation.
        let (channels, _) = store
            .read(ListChannelsRead {
                viewer: owner.clone(),
                owner: owner.clone(),
                params: ChatChannelsParams::default(),
                now_utc: now.clone(),
            })
            .unwrap();
        assert!(
            channels
                .channels
                .iter()
                .any(|c| c.name.as_deref() == Some("general")),
            "{engine:?}: missing #general"
        );

        // 2. Manual channel create, join, leave; auto-channel cannot be manually left.
        let created_ch = store
            .run(ChannelCreateOp {
                here: here.clone(),
                owner: owner.clone(),
                caller: owner.clone(),
                params: ChatChannelCreateParams {
                    id: None,
                    name: "backend-ops".into(),
                    members: vec![],
                },
                now_utc: now.clone(),
            })
            .unwrap()
            .unwrap();
        assert_eq!(created_ch.channel.name.as_deref(), Some("backend-ops"));

        let joined = store
            .run(ChannelMembershipOp {
                owner: owner.clone(),
                caller: ada.clone(),
                params: ChatChannelMembershipParams {
                    channel: "#backend-ops".into(),
                },
                join: true,
                now_utc: now.clone(),
            })
            .unwrap()
            .unwrap();
        assert!(joined.joined);
        assert!(joined.channel.is_member);

        let auto_leave_err = store
            .run(ChannelMembershipOp {
                owner: owner.clone(),
                caller: ada.clone(),
                params: ChatChannelMembershipParams {
                    channel: "#general".into(),
                },
                join: false,
                now_utc: now.clone(),
            })
            .unwrap()
            .unwrap_err();
        assert_eq!(auto_leave_err.code, ErrorCode::InvalidRequest);

        // 3. WATCH-1: channel message without @mention creates NO inbox entry for Ada.
        let m1_id = id(301).as_str().to_owned();
        let sent_unmentioned = store
            .run(ChatSendOp {
                here: here.clone(),
                owner: owner.clone(),
                caller: owner.clone(),
                params: ChatSendParams {
                    id: m1_id.clone(),
                    to: "#backend-ops".into(),
                    kind: "msg".into(),
                    priority: None,
                    subject: Some("Status".into()),
                    body: "Routine update, `@ada` in code and @here do not mention.".into(),
                    meta: Meta::new(),
                    reply_to: None,
                    also_in_channel: false,
                    reason: None,
                    attachment_ids: Vec::new(),
                },
                org_seq: 2,
                now_utc: now.clone(),
                files: org_office::messaging::files::MessageFiles::none(),
            })
            .unwrap()
            .unwrap();
        assert!(sent_unmentioned.result.mentions.is_empty());
        let (ada_inbox_0, _) = store
            .read(ChatInboxRead {
                owner: owner.clone(),
                caller: ada.clone(),
                params: ChatInboxParams::default(),
            })
            .unwrap();
        assert!(
            ada_inbox_0.unwrap().items.is_empty(),
            "{engine:?}: WATCH-1 violated — unmentioned channel post reached inbox"
        );

        // 4. WATCH-2 & WATCH-9: @ada reaches Ada's inbox with delivered_because=mention,
        //    while @nobody is returned in unmatched_mentions.
        let m2_id = id(302).as_str().to_owned();
        let sent_mentioned = store
            .run(ChatSendOp {
                here: here.clone(),
                owner: owner.clone(),
                caller: owner.clone(),
                params: ChatSendParams {
                    id: m2_id.clone(),
                    to: "#backend-ops".into(),
                    kind: "msg".into(),
                    priority: None,
                    subject: Some("Review".into()),
                    body: "Please check this @ada. Also cc @nobody.".into(),
                    meta: Meta::new(),
                    reply_to: None,
                    also_in_channel: false,
                    reason: None,
                    attachment_ids: Vec::new(),
                },
                org_seq: 2,
                now_utc: now.clone(),
                files: org_office::messaging::files::MessageFiles::none(),
            })
            .unwrap()
            .unwrap();
        assert_eq!(sent_mentioned.result.mentions.len(), 1);
        assert_eq!(sent_mentioned.result.mentions[0].employee_id, ada.as_str());
        assert_eq!(sent_mentioned.result.unmatched_mentions, vec!["nobody"]);

        let (ada_inbox_1, _) = store
            .read(ChatInboxRead {
                owner: owner.clone(),
                caller: ada.clone(),
                params: ChatInboxParams {
                    because: Some(DeliveredBecause::Mention),
                    ..Default::default()
                },
            })
            .unwrap();
        let items = ada_inbox_1.unwrap().items;
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, m2_id);
        assert_eq!(items[0].delivered_because, DeliveredBecause::Mention);

        // 5. Thread reply reaches the thread starter with delivered_because=thread_reply.
        let m3_id = id(303).as_str().to_owned();
        let _reply = store
            .run(ChatSendOp {
                here: here.clone(),
                owner: owner.clone(),
                caller: ada.clone(),
                params: ChatSendParams {
                    id: m3_id.clone(),
                    to: "#backend-ops".into(),
                    kind: "reply".into(),
                    priority: None,
                    subject: None,
                    body: "Looks good!".into(),
                    meta: Meta::new(),
                    reply_to: Some(m2_id.clone()),
                    also_in_channel: false,
                    reason: None,
                    attachment_ids: Vec::new(),
                },
                org_seq: 2,
                now_utc: now.clone(),
                files: org_office::messaging::files::MessageFiles::none(),
            })
            .unwrap()
            .unwrap();
        let (owner_inbox, _) = store
            .read(ChatInboxRead {
                owner: owner.clone(),
                caller: owner.clone(),
                params: ChatInboxParams {
                    because: Some(DeliveredBecause::ThreadReply),
                    ..Default::default()
                },
            })
            .unwrap();
        assert_eq!(owner_inbox.unwrap().items[0].id, m3_id);

        // 6. DM open + reuse + handoff send & msg.accept + reactions + read marker + mute.
        let dm1 = store
            .run(DmOpenOp {
                here: here.clone(),
                owner: owner.clone(),
                caller: owner.clone(),
                params: DmOpenParams {
                    participants: vec!["ada".into()],
                    name: None,
                    conversation_id: None,
                },
                now_utc: now.clone(),
            })
            .unwrap()
            .unwrap();
        assert!(dm1.created);
        assert_eq!(dm1.conversation.kind, ConversationKind::Dm);
        let dm2 = store
            .run(DmOpenOp {
                here: here.clone(),
                owner: owner.clone(),
                caller: ada.clone(),
                params: DmOpenParams {
                    participants: vec!["owner".into()],
                    name: None,
                    conversation_id: None,
                },
                now_utc: now.clone(),
            })
            .unwrap()
            .unwrap();
        assert!(!dm2.created);
        assert_eq!(dm2.conversation.id, dm1.conversation.id);

        let handoff_id = id(304).as_str().to_owned();
        let _handoff = store
            .run(ChatSendOp {
                here: here.clone(),
                owner: owner.clone(),
                caller: owner.clone(),
                params: ChatSendParams {
                    id: handoff_id.clone(),
                    to: "ada".into(),
                    kind: "handoff".into(),
                    priority: None,
                    subject: Some("Deploy".into()),
                    body: "Please take over release".into(),
                    meta: Meta::new(),
                    reply_to: None,
                    also_in_channel: false,
                    reason: None,
                    attachment_ids: Vec::new(),
                },
                org_seq: 2,
                now_utc: now.clone(),
                files: org_office::messaging::files::MessageFiles::none(),
            })
            .unwrap()
            .unwrap();

        let accepted = store
            .run(MsgAcceptOp {
                here: here.clone(),
                caller: ada.clone(),
                params: MsgAcceptParams {
                    id: handoff_id.clone(),
                    reason: Some("Taking over now".into()),
                },
                org_seq: 2,
                now_utc: now.clone(),
            })
            .unwrap()
            .unwrap();
        assert_eq!(accepted.accepted_by, ada.as_str());

        let reacted = store
            .run(ChatReactOp {
                here: here.clone(),
                owner: owner.clone(),
                caller: ada.clone(),
                params: ChatReactParams {
                    message_id: handoff_id.clone(),
                    reaction: ReactionKind::Looking,
                    remove: false,
                },
                now_utc: now.clone(),
            })
            .unwrap()
            .unwrap();
        assert!(reacted.present);

        let _marked = store
            .run(ChatMarkReadOp {
                here: here.clone(),
                owner: owner.clone(),
                caller: ada.clone(),
                params: ChatMarkReadParams {
                    conversation_id: dm1.conversation.id.clone(),
                    last_read_message_id: Some(handoff_id.clone()),
                    dry_run: false,
                },
                now_utc: now.clone(),
            })
            .unwrap()
            .unwrap();

        let _muted = store
            .run(ChatMuteOp {
                here: here.clone(),
                owner: owner.clone(),
                caller: ada.clone(),
                params: ChatMuteParams {
                    conversation_id: dm1.conversation.id.clone(),
                    muted_employee_id: owner.as_str().to_owned(),
                    mute_mode: MuteMode::Muted,
                },
                now_utc: now.clone(),
            })
            .unwrap()
            .unwrap();

        let (msgs, _) = store
            .read(ChatMessagesRead {
                owner: owner.clone(),
                caller: ada.clone(),
                params: ChatMessagesParams {
                    conversation: Some(dm1.conversation.id.clone()),
                    ..Default::default()
                },
                now_utc: now.clone(),
            })
            .unwrap();
        let msgs = msgs.unwrap();
        assert_eq!(msgs.messages.len(), 1);
        assert_eq!(msgs.messages[0].reactions.len(), 1);
        assert_eq!(
            msgs.messages[0].acceptance.as_ref().unwrap().accepted_by,
            ada.as_str()
        );

        // 7. Close, delete runtime/<office-id>/ (both office.db and protected/messaging.db),
        //    reopen, and verify full messaging projection rebuilds identically.
        let before_dump = store.dump();
        store.close().unwrap();

        if engine == EngineChoice::Sqlite {
            let office_db = place.config(engine, Opening::Existing).projection_path();
            let protected_db = office_db
                .parent()
                .unwrap()
                .join("protected")
                .join("messaging.db");
            assert!(
                protected_db.exists(),
                "isolated protected/messaging.db must exist separately from office.db"
            );
            std::fs::remove_dir_all(office_db.parent().unwrap()).unwrap();
        }

        let reopened = place.reopen(engine);
        let after_dump = reopened.dump();
        assert_eq!(
            before_dump, after_dump,
            "{engine:?}: deleting runtime folder must rebuild identical messaging projection"
        );
        let (rebuilt_convs, _) = reopened
            .read(ListConversationsRead {
                viewer: ada.clone(),
                owner: owner.clone(),
                params: ChatConversationsParams::default(),
                now_utc: now.clone(),
            })
            .unwrap();
        assert!(rebuilt_convs
            .conversations
            .iter()
            .any(|c| c.id == dm1.conversation.id
                && c.last_read_message_id.as_deref() == Some(handoff_id.as_str())));

        // Verify recipient state preserved as Read after handoff accept + rebuild
        let (ada_all, _) = reopened
            .read(ChatInboxRead {
                owner: owner.clone(),
                caller: ada.clone(),
                params: ChatInboxParams {
                    include_all: true,
                    ..Default::default()
                },
            })
            .unwrap();
        let handoff_row = ada_all
            .unwrap()
            .items
            .into_iter()
            .find(|i| i.id == handoff_id)
            .expect("handoff row rebuilt");
        assert_eq!(handoff_row.state, MessageLifecycleState::Read);
        reopened.close().unwrap();
    }
}

#[test]
fn messaging_pass1_security_mentions_reactions_snooze_and_org_changes_regressions() {
    for engine in ENGINES {
        let place = Place::new();
        let (store, owner, ada) = seed_org_with_two_people(&place, engine);
        let here = identity();
        let now = "2026-11-21T12:00:00.000Z".to_owned();
        let top_team = creation_command().team_id;

        // Add a second non-owner person (`bob`) and discover the seeded HR agent.
        let bob = id(201);
        assert_eq!(
            store
                .run(AddPersonOp {
                    operation_id: id(3),
                    here: here.clone(),
                    owner: owner.clone(),
                    by: Actor::Employee(owner.clone()),
                    command: org_office::organization_family::features::AddPerson::new(
                        bob.clone(),
                        "bob",
                        "Bob",
                        top_team.clone(),
                    ),
                })
                .unwrap()
                .outcome,
            Outcome::Completed
        );
        let (snap, _) = store
            .read(DirectorySnapshot {
                owner: owner.clone(),
            })
            .unwrap();
        let hr_id = Id::parse(
            &snap
                .members
                .iter()
                .find(|m| m.handle.as_deref() == Some("hr"))
                .expect("seeded hr")
                .id,
        )
        .unwrap();

        // Finding #2: non-hiring person (`ada`) cannot bypass HR access via dm.open,
        // chat.send by DM conversation_id, or channel @hr mention.
        let dm_hr_err = store
            .run(DmOpenOp {
                here: here.clone(),
                owner: owner.clone(),
                caller: ada.clone(),
                params: DmOpenParams {
                    participants: vec!["hr".into()],
                    name: None,
                    conversation_id: None,
                },
                now_utc: now.clone(),
            })
            .unwrap()
            .unwrap_err();
        assert_eq!(dm_hr_err.code, ErrorCode::NotAuthorized, "{engine:?}");

        let owner_hr_dm = store
            .run(DmOpenOp {
                here: here.clone(),
                owner: owner.clone(),
                caller: owner.clone(),
                params: DmOpenParams {
                    participants: vec![hr_id.as_str().to_owned()],
                    name: None,
                    conversation_id: None,
                },
                now_utc: now.clone(),
            })
            .unwrap()
            .unwrap();
        let chan_hr_err = store
            .run(ChatSendOp {
                here: here.clone(),
                owner: owner.clone(),
                caller: ada.clone(),
                params: ChatSendParams {
                    id: id(401).as_str().to_owned(),
                    to: "#general".into(),
                    kind: "msg".into(),
                    priority: None,
                    subject: None,
                    body: "Hey @hr hire someone".into(),
                    meta: Meta::new(),
                    reply_to: None,
                    also_in_channel: false,
                    reason: None,
                    attachment_ids: Vec::new(),
                },
                org_seq: 3,
                now_utc: now.clone(),
                files: org_office::messaging::files::MessageFiles::none(),
            })
            .unwrap()
            .unwrap_err();
        assert_eq!(chan_hr_err.code, ErrorCode::NotAuthorized, "{engine:?}");

        // Finding #3: non-member (`bob`) cannot react, mark_read, or mute on owner<->ada DM.
        let dm = store
            .run(DmOpenOp {
                here: here.clone(),
                owner: owner.clone(),
                caller: owner.clone(),
                params: DmOpenParams {
                    participants: vec!["ada".into()],
                    name: None,
                    conversation_id: None,
                },
                now_utc: now.clone(),
            })
            .unwrap()
            .unwrap();
        let dm_msg_id = id(402).as_str().to_owned();
        let sent_dm = store
            .run(ChatSendOp {
                here: here.clone(),
                owner: owner.clone(),
                caller: owner.clone(),
                params: ChatSendParams {
                    id: dm_msg_id.clone(),
                    to: dm.conversation.id.clone(),
                    kind: "msg".into(),
                    priority: None,
                    subject: None,
                    body: "Hello @adam and ``@ada`` (not mentions), real @ada!".into(),
                    meta: Meta::new(),
                    reply_to: None,
                    also_in_channel: false,
                    reason: None,
                    attachment_ids: Vec::new(),
                },
                org_seq: 3,
                now_utc: now.clone(),
                files: org_office::messaging::files::MessageFiles::none(),
            })
            .unwrap()
            .unwrap();
        // Finding #4: @adam is unmatched (no prefix match to ada), ``@ada`` ignored in code span.
        assert_eq!(sent_dm.result.mentions.len(), 1, "{engine:?}");
        assert_eq!(sent_dm.result.mentions[0].handle, "ada");
        assert_eq!(sent_dm.result.unmatched_mentions, vec!["adam"]);

        assert_eq!(
            store
                .run(ChatReactOp {
                    here: here.clone(),
                    owner: owner.clone(),
                    caller: bob.clone(),
                    params: ChatReactParams {
                        message_id: dm_msg_id.clone(),
                        reaction: ReactionKind::Looking,
                        remove: false,
                    },
                    now_utc: now.clone(),
                })
                .unwrap()
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
        assert_eq!(
            store
                .run(ChatMarkReadOp {
                    here: here.clone(),
                    owner: owner.clone(),
                    caller: bob.clone(),
                    params: ChatMarkReadParams {
                        conversation_id: dm.conversation.id.clone(),
                        last_read_message_id: Some(dm_msg_id.clone()),
                        dry_run: false,
                    },
                    now_utc: now.clone(),
                })
                .unwrap()
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
        assert_eq!(
            store
                .run(ChatMuteOp {
                    here: here.clone(),
                    owner: owner.clone(),
                    caller: bob.clone(),
                    params: ChatMuteParams {
                        conversation_id: dm.conversation.id.clone(),
                        muted_employee_id: owner.as_str().to_owned(),
                        mute_mode: MuteMode::Muted,
                    },
                    now_utc: now.clone(),
                })
                .unwrap()
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );

        // Finding #5: react -> remove -> re-react succeeds (no AlreadyExists on put vs create).
        for remove in [false, true, false] {
            let r = store
                .run(ChatReactOp {
                    here: here.clone(),
                    owner: owner.clone(),
                    caller: ada.clone(),
                    params: ChatReactParams {
                        message_id: dm_msg_id.clone(),
                        reaction: ReactionKind::Looking,
                        remove,
                    },
                    now_utc: now.clone(),
                })
                .unwrap()
                .unwrap();
            assert_eq!(r.present, !remove);
        }

        // Finding #6: two due snoozes in the same conversation wake cleanly in one commit.
        let dm_msg_2 = id(403).as_str().to_owned();
        let _ = store
            .run(ChatSendOp {
                here: here.clone(),
                owner: owner.clone(),
                caller: owner.clone(),
                params: ChatSendParams {
                    id: dm_msg_2.clone(),
                    to: dm.conversation.id.clone(),
                    kind: "msg".into(),
                    priority: None,
                    subject: None,
                    body: "Second DM".into(),
                    meta: Meta::new(),
                    reply_to: None,
                    also_in_channel: false,
                    reason: None,
                    attachment_ids: Vec::new(),
                },
                org_seq: 3,
                now_utc: now.clone(),
                files: org_office::messaging::files::MessageFiles::none(),
            })
            .unwrap()
            .unwrap();
        for mid in [&dm_msg_id, &dm_msg_2] {
            let _ = store
                .run(MailboxUpdateStateOp {
                    here: here.clone(),
                    input: org_office::testkit_support::store_open::messaging_ops::LocalTransitionInput {
                        id: mid.clone(),
                        to_member_id: ada.clone(),
                        state: MessageLifecycleState::Snoozed,
                        snoozed_until: Some("2026-11-21T12:05:00.000Z".into()),
                        snooze_after_task: false,
                        reason: None,
                        now_utc: now.clone(),
                        owner_office_id: here.office_id.clone(),
                        origin_incarnation: here.journal_incarnation.to_string(),
                        org_position: org_proto::org::Position { epoch: 1, seq: 3 },
                        expired_waiting_person: false,
                    },
                })
                .unwrap()
                .unwrap();
        }
        let woken = store
            .run(MailboxCheckNotifyOp {
                here: here.clone(),
                to_member_id: ada.clone(),
                after_task: false,
                urgent_only: false,
                sweep_before_utc: "2026-11-21T12:00:00.000Z".into(),
                now_utc: "2026-11-21T12:10:00.000Z".into(),
            })
            .unwrap()
            .unwrap();
        assert_eq!(
            woken.len(),
            2,
            "{engine:?}: both snoozed messages must wake"
        );

        // Finding #8: OrgChangesAfterRead returns real MemberUpserted (never Unknown/stub).
        let (org_changes, _) = store
            .read(OrgChangesAfterRead {
                owner: owner.clone(),
                after_seq: 1,
                limit: 100,
            })
            .unwrap();
        assert!(!org_changes.is_empty(), "{engine:?}");
        for ev in &org_changes {
            assert!(
                !ev.changes
                    .iter()
                    .any(|c| matches!(c, org_proto::org::OrgChange::Unknown)),
                "OrgChangesAfterRead must not emit Unknown: {ev:?}"
            );
        }
        assert!(
            org_changes.iter().any(|ev| ev.changes.iter().any(|c| matches!(
                c,
                org_proto::org::OrgChange::MemberUpserted { member } if member.id == bob.as_str() && member.name == "Bob"
            ))),
            "expected populated MemberUpserted for Bob: {org_changes:?}"
        );
        let _ = owner_hr_dm;
        store.close().unwrap();
    }
}

#[test]
fn messaging_pass2_dm_hijack_and_handoff_hr_guard_regressions() {
    for engine in ENGINES {
        let place = Place::new();
        let (store, owner, ada) = seed_org_with_two_people(&place, engine);
        let here = identity();
        let now = "2026-11-21T12:00:00.000Z".to_owned();
        let top_team = creation_command().team_id;
        let bob = id(202);
        assert_eq!(
            store
                .run(AddPersonOp {
                    operation_id: id(4),
                    here: here.clone(),
                    owner: owner.clone(),
                    by: Actor::Employee(owner.clone()),
                    command: org_office::organization_family::features::AddPerson::new(
                        bob.clone(),
                        "bob",
                        "Bob",
                        top_team,
                    ),
                })
                .unwrap()
                .outcome,
            Outcome::Completed
        );
        let (snap, _) = store
            .read(DirectorySnapshot {
                owner: owner.clone(),
            })
            .unwrap();
        let hr_id = Id::parse(
            &snap
                .members
                .iter()
                .find(|m| m.handle.as_deref() == Some("hr"))
                .unwrap()
                .id,
        )
        .unwrap();

        let dm = store
            .run(DmOpenOp {
                here: here.clone(),
                owner: owner.clone(),
                caller: owner.clone(),
                params: DmOpenParams {
                    participants: vec!["ada".into()],
                    name: None,
                    conversation_id: None,
                },
                now_utc: now.clone(),
            })
            .unwrap()
            .unwrap();

        // Non-member dm.open with colliding conversation_id (existing DM or virtual #general)
        // is refused with AlreadyExists and never overwrites conversation.json.
        let hijack_dm_err = store
            .run(DmOpenOp {
                here: here.clone(),
                owner: owner.clone(),
                caller: bob.clone(),
                params: DmOpenParams {
                    participants: vec!["ada".into()],
                    name: None,
                    conversation_id: Some(dm.conversation.id.clone()),
                },
                now_utc: now.clone(),
            })
            .unwrap()
            .unwrap_err();
        assert_eq!(hijack_dm_err.code, ErrorCode::AlreadyExists, "{engine:?}");

        let (chs, _) = store
            .read(ListChannelsRead {
                viewer: bob.clone(),
                owner: owner.clone(),
                params: ChatChannelsParams::default(),
                now_utc: now.clone(),
            })
            .unwrap();
        let general_id = chs
            .channels
            .iter()
            .find(|c| c.name.as_deref() == Some("general"))
            .unwrap()
            .id
            .clone();
        let hijack_ch_err = store
            .run(DmOpenOp {
                here: here.clone(),
                owner: owner.clone(),
                caller: bob.clone(),
                params: DmOpenParams {
                    participants: vec!["ada".into()],
                    name: None,
                    conversation_id: Some(general_id.clone()),
                },
                now_utc: now.clone(),
            })
            .unwrap()
            .unwrap_err();
        assert_eq!(hijack_ch_err.code, ErrorCode::AlreadyExists, "{engine:?}");

        // Channel create with explicit id colliding with #general or existing DM is refused AlreadyExists.
        for colliding_id in [general_id, dm.conversation.id.clone()] {
            let err = store
                .run(ChannelCreateOp {
                    here: here.clone(),
                    owner: owner.clone(),
                    caller: bob.clone(),
                    params: ChatChannelCreateParams {
                        id: Some(colliding_id),
                        name: "hijack-attempt".into(),
                        members: Vec::new(),
                    },
                    now_utc: now.clone(),
                })
                .unwrap()
                .unwrap_err();
            assert_eq!(err.code, ErrorCode::AlreadyExists, "{engine:?}");
        }

        // Channel handoff with meta.card.addressed_to = hr_id cannot bypass HIRE-2.
        let mut handoff_meta = Meta::new();
        handoff_meta.insert(
            "card".into(),
            serde_json::json!({ "addressed_to": hr_id.as_str() }),
        );
        let handoff_hr_err = store
            .run(ChatSendOp {
                here: here.clone(),
                owner: owner.clone(),
                caller: ada.clone(),
                params: ChatSendParams {
                    id: id(404).as_str().to_owned(),
                    to: "#general".into(),
                    kind: "handoff".into(),
                    priority: None,
                    subject: Some("Bypass attempt".into()),
                    body: "No mention in body".into(),
                    meta: handoff_meta,
                    reply_to: None,
                    also_in_channel: false,
                    reason: None,
                    attachment_ids: Vec::new(),
                },
                org_seq: 3,
                now_utc: now.clone(),
                files: org_office::messaging::files::MessageFiles::none(),
            })
            .unwrap()
            .unwrap_err();
        assert_eq!(handoff_hr_err.code, ErrorCode::NotAuthorized, "{engine:?}");
        store.close().unwrap();
    }
}

#[test]
fn b7_structured_asks_all_question_types_defaults_required_withdraw_and_rebuild() {
    use org_proto::ask::{
        AskOption, AskQuestion, AskQuestionType, AskStatus, ChatAnswerParams, ChatAskParams,
        MsgWithdrawParams, QuestionAnswer,
    };

    for engine in ENGINES {
        let place = Place::new();
        let (store, owner, ada) = seed_org_with_two_people(&place, engine);
        let here = identity();
        let t0_ms: u64 = 1_700_000_010_000;
        let now = org_office::format_rfc3339_ms(t0_ms);

        // ASK-1: wait_ms above maximum (3_600_000 ms) is refused.
        let bad_wait = store
            .run(MsgAskOp {
                here: here.clone(),
                owner: owner.clone(),
                caller: ada.clone(),
                params: ChatAskParams {
                    id: id(500).as_str().to_owned(),
                    to: "owner".into(),
                    body: None,
                    subject: None,
                    questions: vec![AskQuestion {
                        id: "q1".into(),
                        question_type: AskQuestionType::YesNo,
                        prompt: "Proceed?".into(),
                        options: Vec::new(),
                        allow_multiple: false,
                        default_value: Some("yes".into()),
                        optional: false,
                    }],
                    required: false,
                    wait_ms: Some(4_000_000),
                    hold_ms: Some(0),
                    blocks: None,
                    approval: false,
                    addressed_to: None,
                    meta: Meta::new(),
                    attachment_ids: Vec::new(),
                },
                org_seq: 2,
                now_ms: t0_ms,
                files: org_office::messaging::files::MessageFiles::none(),
            })
            .unwrap()
            .unwrap_err();
        assert_eq!(bad_wait.code, ErrorCode::InvalidRequest, "{engine:?}");

        // 1. Send a 4-question ask covering single choice, multi choice, yes_no, free_text.
        let ask_id = id(501).as_str().to_owned();
        let questions = vec![
            AskQuestion {
                id: "env".into(),
                question_type: AskQuestionType::Choice,
                prompt: "Target environment?".into(),
                options: vec![
                    AskOption {
                        id: "dev".into(),
                        label: "Dev".into(),
                        description: Some("Staging".into()),
                    },
                    AskOption {
                        id: "prod".into(),
                        label: "Prod".into(),
                        description: None,
                    },
                ],
                allow_multiple: false,
                default_value: Some("dev".into()),
                optional: false,
            },
            AskQuestion {
                id: "checks".into(),
                question_type: AskQuestionType::Choice,
                prompt: "Which checks?".into(),
                options: vec![
                    AskOption {
                        id: "lint".into(),
                        label: "Lint".into(),
                        description: None,
                    },
                    AskOption {
                        id: "e2e".into(),
                        label: "E2E".into(),
                        description: None,
                    },
                ],
                allow_multiple: true,
                default_value: None,
                optional: false,
            },
            AskQuestion {
                id: "notify".into(),
                question_type: AskQuestionType::YesNo,
                prompt: "Notify channel?".into(),
                options: Vec::new(),
                allow_multiple: false,
                default_value: Some("no".into()),
                optional: false,
            },
            AskQuestion {
                id: "notes".into(),
                question_type: AskQuestionType::FreeText,
                prompt: "Release notes?".into(),
                options: Vec::new(),
                allow_multiple: false,
                default_value: None,
                optional: false,
            },
        ];
        let committed = store
            .run(MsgAskOp {
                here: here.clone(),
                owner: owner.clone(),
                caller: ada.clone(),
                params: ChatAskParams {
                    id: ask_id.clone(),
                    to: "owner".into(),
                    body: Some("Release configuration".into()),
                    subject: Some("Release config".into()),
                    questions,
                    required: false,
                    wait_ms: Some(10_000),
                    hold_ms: Some(0),
                    blocks: Some("T-46".into()),
                    approval: false,
                    addressed_to: None,
                    meta: Meta::new(),
                    attachment_ids: Vec::new(),
                },
                org_seq: 2,
                now_ms: t0_ms,
                files: org_office::messaging::files::MessageFiles::none(),
            })
            .unwrap()
            .unwrap();
        assert_eq!(committed.result.status, AskStatus::Waiting);

        // ASK-2: after wait_ms on non-required ask, inspect returns TimedOut + defaults!
        let (timed_out, _) = store
            .read(InspectAskStatusRead {
                owner: owner.clone(),
                caller: ada.clone(),
                ask_id: ask_id.clone(),
                now_ms: t0_ms + 10_000,
            })
            .unwrap();
        let timed_out = timed_out.unwrap();
        assert_eq!(timed_out.status, AskStatus::TimedOut, "{engine:?}");
        assert_eq!(timed_out.defaults.len(), 2, "{timed_out:?}");

        // Owner answers all 4 questions together (partial answers refused first).
        let partial_err = store
            .run(ChatAnswerOp {
                here: here.clone(),
                owner: owner.clone(),
                caller: owner.clone(),
                params: ChatAnswerParams {
                    ask_id: ask_id.clone(),
                    answers: vec![QuestionAnswer {
                        question_id: "env".into(),
                        option_ids: vec!["prod".into()],
                        text: None,
                    }],
                    option_id: None,
                    text: None,
                    attachment_ids: Vec::new(),
                },
                reply_id: id(502).as_str().to_owned(),
                org_seq: 2,
                now_utc: now.clone(),
                files: org_office::messaging::files::MessageFiles::none(),
            })
            .unwrap()
            .unwrap_err();
        assert_eq!(partial_err.code, ErrorCode::InvalidRequest);

        let answered = store
            .run(ChatAnswerOp {
                here: here.clone(),
                owner: owner.clone(),
                caller: owner.clone(),
                params: ChatAnswerParams {
                    ask_id: ask_id.clone(),
                    answers: vec![
                        QuestionAnswer {
                            question_id: "env".into(),
                            option_ids: vec!["prod".into()],
                            text: None,
                        },
                        QuestionAnswer {
                            question_id: "checks".into(),
                            option_ids: vec!["lint".into(), "e2e".into()],
                            text: None,
                        },
                        QuestionAnswer {
                            question_id: "notify".into(),
                            option_ids: vec!["yes".into()],
                            text: None,
                        },
                        QuestionAnswer {
                            question_id: "notes".into(),
                            option_ids: Vec::new(),
                            text: Some("Ship it!".into()),
                        },
                    ],
                    option_id: None,
                    text: None,
                    attachment_ids: Vec::new(),
                },
                reply_id: id(503).as_str().to_owned(),
                org_seq: 2,
                now_utc: now.clone(),
                files: org_office::messaging::files::MessageFiles::none(),
            })
            .unwrap()
            .unwrap();
        assert_eq!(answered.result.answer.answers.len(), 4);

        // 2. Required ask with default_value: after wait_ms returns RequiredPending and NEVER defaults!
        let req_ask_id = id(504).as_str().to_owned();
        let _ = store
            .run(MsgAskOp {
                here: here.clone(),
                owner: owner.clone(),
                caller: ada.clone(),
                params: ChatAskParams {
                    id: req_ask_id.clone(),
                    to: "owner".into(),
                    body: None,
                    subject: Some("Required confirmation".into()),
                    questions: vec![AskQuestion {
                        id: "ok".into(),
                        question_type: AskQuestionType::YesNo,
                        prompt: "Delete production cluster?".into(),
                        options: Vec::new(),
                        allow_multiple: false,
                        default_value: Some("no".into()),
                        optional: false,
                    }],
                    required: true,
                    wait_ms: Some(5_000),
                    hold_ms: Some(0),
                    blocks: None,
                    approval: true,
                    addressed_to: None,
                    meta: Meta::new(),
                    attachment_ids: Vec::new(),
                },
                org_seq: 2,
                now_ms: t0_ms,
                files: org_office::messaging::files::MessageFiles::none(),
            })
            .unwrap()
            .unwrap();
        let (req_status, _) = store
            .read(InspectAskStatusRead {
                owner: owner.clone(),
                caller: ada.clone(),
                ask_id: req_ask_id.clone(),
                now_ms: t0_ms + 5_000,
            })
            .unwrap();
        let req_status = req_status.unwrap();
        assert_eq!(req_status.status, AskStatus::RequiredPending, "{engine:?}");
        assert!(
            req_status.defaults.is_empty(),
            "ADR-0020 decision 14: defaults must NEVER stand in for required asks"
        );
        assert!(req_status.required_notice.is_some());

        // Withdraw required ask cleanly.
        let wd = store
            .run(MsgWithdrawOp {
                here: here.clone(),
                owner: owner.clone(),
                caller: ada.clone(),
                params: MsgWithdrawParams {
                    ask_id: req_ask_id.clone(),
                    reason: Some("Canceled plan".into()),
                },
                org_seq: 2,
                now_utc: now.clone(),
                removed_offices: std::collections::BTreeSet::new(),
            })
            .unwrap()
            .unwrap();
        assert_eq!(wd.result.withdrawal.by, ada.as_str());

        // 3. Delete runtime and rebuild: verify answer + withdrawal survive identically!
        let before = store.dump();
        store.close().unwrap();
        if engine == EngineChoice::Sqlite {
            let office_db = place.config(engine, Opening::Existing).projection_path();
            std::fs::remove_dir_all(office_db.parent().unwrap()).unwrap();
        }
        let reopened = place.reopen(engine);
        assert_eq!(before, reopened.dump(), "{engine:?}: ask rebuild mismatch");
        let (rebuilt_msgs, _) = reopened
            .read(ChatMessagesRead {
                owner: owner.clone(),
                caller: owner.clone(),
                params: ChatMessagesParams {
                    conversation: Some(committed.result.conversation_id.clone()),
                    ..Default::default()
                },
                now_utc: now,
            })
            .unwrap();
        let rebuilt_msgs = rebuilt_msgs.unwrap();
        let ans_view = rebuilt_msgs
            .messages
            .iter()
            .find(|m| m.id == ask_id)
            .expect("answered ask rebuilt");
        assert_eq!(ans_view.answer.as_ref().unwrap().answers.len(), 4);
        let wd_view = rebuilt_msgs
            .messages
            .iter()
            .find(|m| m.id == req_ask_id)
            .expect("withdrawn ask rebuilt");
        assert_eq!(wd_view.withdrawal.as_ref().unwrap().by, ada.as_str());
        reopened.close().unwrap();
    }
}

fn snooze_transition_op(
    here: &OfficeIdentity,
    msg_id: &str,
    member: &Id,
    state: MessageLifecycleState,
    until: Option<&str>,
    note: Option<&str>,
    now_utc: &str,
) -> MailboxUpdateStateOp {
    MailboxUpdateStateOp {
        here: here.clone(),
        input: org_office::LocalTransitionInput {
            id: msg_id.to_owned(),
            to_member_id: member.clone(),
            state,
            snoozed_until: until.map(str::to_owned),
            snooze_after_task: false,
            reason: note.map(str::to_owned),
            now_utc: now_utc.to_owned(),
            owner_office_id: here.office_id.clone(),
            origin_incarnation: "inc-1".into(),
            org_position: org_proto::Position { epoch: 1, seq: 2 },
            expired_waiting_person: false,
        },
    }
}

fn defer_transition_op(
    here: &OfficeIdentity,
    msg_id: &str,
    member: &Id,
    until: Option<&str>,
    note: Option<&str>,
    now_utc: &str,
) -> MailboxDeferOp {
    MailboxDeferOp {
        here: here.clone(),
        input: snooze_transition_op(
            here,
            msg_id,
            member,
            MessageLifecycleState::Snoozed,
            until,
            note,
            now_utc,
        )
        .input,
    }
}

fn add_third_person(store: &OfficeStore, here: &OfficeIdentity, owner: &Id) -> Id {
    let bob = id(201);
    let top_team = creation_command().team_id;
    let add_bob = AddPersonOp {
        operation_id: id(3),
        here: here.clone(),
        owner: owner.clone(),
        by: Actor::Employee(owner.clone()),
        command: org_office::organization_family::features::AddPerson::new(
            bob.clone(),
            "bob",
            "Bob",
            top_team,
        ),
    };
    assert_eq!(store.run(add_bob).unwrap().outcome, Outcome::Completed);
    bob
}

fn send_snooze_fixtures(
    store: &OfficeStore,
    here: &OfficeIdentity,
    owner: &Id,
    ada: &Id,
    t0_ms: u64,
    now: &str,
) -> (String, String) {
    let ch_msg_id = id(601).as_str().to_owned();
    store
        .run(ChatSendOp {
            here: here.clone(),
            owner: owner.clone(),
            caller: ada.clone(),
            params: ChatSendParams {
                id: ch_msg_id.clone(),
                to: "#general".into(),
                kind: "msg".into(),
                priority: None,
                subject: Some("Channel mention".into()),
                body: "Checking with @owner and @bob".into(),
                meta: Meta::new(),
                reply_to: None,
                also_in_channel: false,
                reason: None,
                attachment_ids: Vec::new(),
            },
            org_seq: 2,
            now_utc: now.to_owned(),
            files: org_office::messaging::files::MessageFiles::none(),
        })
        .unwrap()
        .unwrap();

    let ask_id = id(602).as_str().to_owned();
    store
        .run(MsgAskOp {
            here: here.clone(),
            owner: owner.clone(),
            caller: ada.clone(),
            params: org_proto::ask::ChatAskParams {
                id: ask_id.clone(),
                to: "owner".into(),
                body: None,
                subject: Some("Timed ask".into()),
                questions: vec![org_proto::ask::AskQuestion {
                    id: "q1".into(),
                    question_type: org_proto::ask::AskQuestionType::YesNo,
                    prompt: "Approve deploy?".into(),
                    options: Vec::new(),
                    allow_multiple: false,
                    default_value: None,
                    optional: false,
                }],
                required: false,
                wait_ms: Some(300_000),
                hold_ms: Some(0),
                blocks: None,
                approval: false,
                addressed_to: None,
                meta: Meta::new(),
                attachment_ids: Vec::new(),
            },
            org_seq: 2,
            now_ms: t0_ms,
            files: org_office::messaging::files::MessageFiles::none(),
        })
        .unwrap()
        .unwrap();
    (ch_msg_id, ask_id)
}

fn assert_snoozed_owner_views(
    store: &OfficeStore,
    owner: &Id,
    bob: &Id,
    ask_id: &str,
    ch_msg_id: &str,
    engine: EngineChoice,
) {
    let (active, _) = store
        .read(ChatInboxRead {
            owner: owner.clone(),
            caller: owner.clone(),
            params: ChatInboxParams::default(),
        })
        .unwrap();
    assert!(active.unwrap().items.is_empty(), "{engine:?}");

    // Multi-recipient isolation: Bob still has ch_msg_id Unread after Owner snoozed it.
    let (bob_active, _) = store
        .read(ChatInboxRead {
            owner: owner.clone(),
            caller: bob.clone(),
            params: ChatInboxParams::default(),
        })
        .unwrap();
    let bob_items = bob_active.unwrap().items;
    assert_eq!(bob_items.len(), 1, "{engine:?}: bob must stay unaffected");
    assert_eq!(bob_items[0].id, ch_msg_id);
    assert_eq!(bob_items[0].state, MessageLifecycleState::Unread);

    let (owner_entries, _) = store
        .read(OpenOwnerInboxRead {
            owner: owner.clone(),
            limit: 50,
        })
        .unwrap();
    assert!(owner_entries.is_empty(), "{engine:?}");

    // Multi-page Snoozed pagination where local_seq (ch=1, ask=2) opposes snoozed_until (ask sooner).
    let (p1, _) = store
        .read(ChatInboxRead {
            owner: owner.clone(),
            caller: owner.clone(),
            params: ChatInboxParams {
                state: Some(MessageLifecycleState::Snoozed),
                limit: Some(1),
                ..Default::default()
            },
        })
        .unwrap();
    let p1 = p1.unwrap();
    assert_eq!(p1.items.len(), 1, "{engine:?}");
    assert_eq!(p1.items[0].id, ask_id, "soonest snoozed_until first");
    assert_eq!(p1.next_cursor, Some(1));

    let (p2, _) = store
        .read(ChatInboxRead {
            owner: owner.clone(),
            caller: owner.clone(),
            params: ChatInboxParams {
                state: Some(MessageLifecycleState::Snoozed),
                limit: Some(1),
                cursor: p1.next_cursor,
                ..Default::default()
            },
        })
        .unwrap();
    let p2 = p2.unwrap();
    assert_eq!(p2.items.len(), 1, "{engine:?}");
    assert_eq!(p2.items[0].id, ch_msg_id);
    assert_eq!(p2.items[0].snooze_note.as_deref(), Some("check later"));
    assert_eq!(p2.next_cursor, None);

    // Unknown state filter is refused with InvalidRequest.
    let (unknown_res, _) = store
        .read(ChatInboxRead {
            owner: owner.clone(),
            caller: owner.clone(),
            params: ChatInboxParams {
                state: Some(MessageLifecycleState::Unknown),
                ..Default::default()
            },
        })
        .unwrap();
    assert_eq!(unknown_res.unwrap_err().code, ErrorCode::InvalidRequest);
}

#[test]
fn task81_snooze_unsnooze_isolation_ask_expiry_and_rebuild() {
    for engine in ENGINES {
        let place = Place::new();
        let (store, owner, ada) = seed_org_with_two_people(&place, engine);
        let here = identity();
        let bob = add_third_person(&store, &here, &owner);
        let t0_ms = 1_795_262_400_000u64;
        let now = org_office::format_rfc3339_ms(t0_ms);
        let (ch_msg_id, ask_id) = send_snooze_fixtures(&store, &here, &owner, &ada, t0_ms, &now);

        let later = org_office::format_rfc3339_ms(t0_ms + 600_000);
        let sooner = org_office::format_rfc3339_ms(t0_ms + 120_000);
        let exact_exp = org_office::format_rfc3339_ms(t0_ms + 86_400_000);
        let after_exp = org_office::format_rfc3339_ms(t0_ms + 90_000_000);

        // Snoozing at exact expires_at OR after already expired is refused via MailboxDeferOp.
        let err_eq = store
            .run(defer_transition_op(
                &here,
                &ask_id,
                &owner,
                Some(&exact_exp),
                None,
                &now,
            ))
            .unwrap()
            .unwrap_err();
        assert_eq!(err_eq.code, ErrorCode::InvalidRequest, "{engine:?}");
        let err_expired = store
            .run(defer_transition_op(
                &here,
                &ask_id,
                &owner,
                Some(&after_exp),
                None,
                &exact_exp,
            ))
            .unwrap()
            .unwrap_err();
        assert_eq!(err_expired.code, ErrorCode::InvalidRequest, "{engine:?}");

        let snoozed_ch = store
            .run(defer_transition_op(
                &here,
                &ch_msg_id,
                &owner,
                Some(&later),
                Some("check later"),
                &now,
            ))
            .unwrap()
            .unwrap();
        assert_eq!(
            (snoozed_ch.state, snoozed_ch.triage_gen),
            (MessageLifecycleState::Snoozed, 1)
        );
        let snoozed_ask = store
            .run(defer_transition_op(
                &here,
                &ask_id,
                &owner,
                Some(&sooner),
                Some("before expiry"),
                &now,
            ))
            .unwrap()
            .unwrap();
        assert_eq!(snoozed_ask.state, MessageLifecycleState::Snoozed);

        assert_snoozed_owner_views(&store, &owner, &bob, &ask_id, &ch_msg_id, engine);

        let before = store.dump();
        store.close().unwrap();
        if engine == EngineChoice::Sqlite {
            let office_db = place.config(engine, Opening::Existing).projection_path();
            std::fs::remove_dir_all(office_db.parent().unwrap()).unwrap();
        }
        let reopened = place.reopen(engine);
        assert_eq!(
            before,
            reopened.dump(),
            "{engine:?}: snooze rebuild mismatch"
        );

        let unsnoozed = reopened
            .run(MailboxUnsnoozeOp {
                here: here.clone(),
                input: snooze_transition_op(
                    &here,
                    &ask_id,
                    &owner,
                    MessageLifecycleState::Unread,
                    None,
                    None,
                    &now,
                )
                .input,
            })
            .unwrap()
            .unwrap();
        assert_eq!(unsnoozed.state, MessageLifecycleState::Unread);
        assert_eq!(unsnoozed.triage_gen, 2);
        assert_eq!(unsnoozed.snoozed_until, None);
        assert_eq!(unsnoozed.snooze_note, None);

        let (owner_after, _) = reopened
            .read(OpenOwnerInboxRead {
                owner: owner.clone(),
                limit: 50,
            })
            .unwrap();
        assert_eq!(owner_after.len(), 1);
        assert_eq!(owner_after[0].header.id, ask_id);
        reopened.close().unwrap();
    }
}
