//! The `caller ∩ tess` scope against a real office through the production
//! [`ChiefOfStaffGate`] (`TASK-80.1`): acting for the owner she never shows, opens or touches
//! another member's DM with her, though the owner's own list holds every DM in the
//! organization; acting for a member she never opens the owner's DM with her; and acting for
//! nobody she touches no DM at all.

use std::sync::Arc;

use org_office::rules::OrgCommand;
use org_office::{Office, TestOffice};
use org_proto::chat::{
    ChatConversations, ChatConversationsParams, ChatInbox, ChatInboxParams, ChatMessages,
    ChatMessagesParams, ChatSendParams, DmOpen, DmOpenParams,
};
use org_proto::org::MemberAddPersonParams;
use org_proto::{Call, ErrorCode, Outcome, Request, Response};
use serde_json::{json, Value};

use super::{scope, ChiefOfStaffPort, Delegation};
use crate::chief_of_staff_port::ChiefOfStaffGate;

fn next_id() -> String {
    let ms = org_office::organization_family::testkit::CREATION_AT.0;
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(7000);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mut bytes = [0u8; 16];
    bytes[0..6].copy_from_slice(&ms.to_be_bytes()[2..8]);
    bytes[6] = 0x70 | ((seq >> 8) as u8 & 0x0f);
    bytes[7] = (seq & 0xff) as u8;
    bytes[8] = 0x80;
    bytes[15] = (seq & 0xff) as u8;
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

/// An office with its owner, Tess, a second person Bob, and a DM with Tess for each person,
/// each holding one message from that person to her.
struct World {
    office: Arc<Office>,
    gate: ChiefOfStaffGate,
    owner: String,
    tess: String,
    bob: String,
    owner_dm: String,
    bob_dm: String,
    owner_message: String,
    bob_message: String,
    _tmp: tempfile::TempDir,
}

fn world() -> World {
    let tmp = tempfile::tempdir().unwrap();
    let office = Arc::new(Office::for_test(TestOffice::default()));
    let owner = office.owner_member_id().to_owned();
    let tess = office
        .chief_of_staff()
        .unwrap()
        .expect("Tess")
        .id
        .to_string();
    let bob = next_id();
    let team = office.org_snapshot().unwrap().teams[0].id.clone();
    office
        .submit_org(
            &owner,
            OrgCommand::MemberAddPerson(MemberAddPersonParams {
                command_id: next_id(),
                member_id: bob.clone(),
                name: "Bob".into(),
                handle: Some("bob".into()),
                home_team_id: Some(team),
                contact: None,
            }),
        )
        .unwrap()
        .unwrap();
    let sessions = Arc::new(org_office_sessions::SessionManager::new(
        Arc::clone(&office),
        org_office_sessions::SessionManagerConfig::production(tmp.path().join("office.sock")),
    ));
    let gate = ChiefOfStaffGate::new(Arc::clone(&office), sessions);
    let (owner_dm, owner_message) = dm_with(&office, &owner, &tess, "the owner's private plan");
    let (bob_dm, bob_message) = dm_with(&office, &bob, &tess, "bob's private worry");
    World {
        office,
        gate,
        owner,
        tess,
        bob,
        owner_dm,
        bob_dm,
        owner_message,
        bob_message,
        _tmp: tmp,
    }
}

/// Opens `from`'s DM with `to` and sends one message into it: `(conversation, message)`.
fn dm_with(office: &Office, from: &str, to: &str, body: &str) -> (String, String) {
    let opened = office
        .dm_open(
            from,
            DmOpenParams {
                participants: vec![to.to_owned()],
                name: None,
                conversation_id: None,
            },
        )
        .unwrap()
        .unwrap();
    let sent = office
        .chat_send(
            from,
            ChatSendParams {
                id: next_id(),
                to: to.to_owned(),
                kind: "msg".into(),
                priority: None,
                subject: None,
                body: body.to_owned(),
                meta: Default::default(),
                reply_to: None,
                also_in_channel: false,
                reason: None,
            },
        )
        .unwrap()
        .unwrap();
    assert_eq!(
        sent.message.conversation_id.as_str(),
        opened.conversation.id
    );
    (opened.conversation.id, sent.message.id)
}

impl World {
    fn acting_for(&self, caller: &str) -> Delegation {
        Delegation {
            caller_id: Some(caller.to_owned()),
            caller: self.office.caller_authority(caller).unwrap(),
        }
    }

    fn request<C: Call>(&self, params: &C::Params) -> Request {
        Request::new::<C>(5, params).unwrap()
    }

    /// What `call` answers Tess herself, narrowed to what `delegation` may see.
    fn narrowed<C: Call>(
        &self,
        params: C::Params,
        delegation: &Delegation,
        call: impl FnOnce(&Office, &str, C::Params) -> C::Result,
    ) -> Response {
        let request = self.request::<C>(&params);
        let answer = call(&self.office, &self.tess, params);
        let response = Response::ok::<C>(request.id, &answer).unwrap();
        scope::narrow(&request, &self.gate, &self.tess, delegation, response)
    }

    fn admit(&self, method: &str, params: Value, delegation: &Delegation) -> Result<(), Response> {
        let request = Request {
            id: 6,
            method: method.into(),
            principal: None,
            params,
        };
        scope::admit(&request, &self.gate, &self.tess, delegation)
    }
}

fn result(response: &Response) -> &Value {
    match &response.outcome {
        Outcome::Result(value) => value,
        Outcome::Error(err) => panic!("expected a result: {err:?}"),
    }
}

fn error_code(response: &Response) -> Option<ErrorCode> {
    match &response.outcome {
        Outcome::Error(err) => Some(err.code),
        Outcome::Result(_) => None,
    }
}

fn refused(admitted: &Result<(), Response>) -> bool {
    matches!(admitted, Err(r) if error_code(r) == Some(ErrorCode::NotAuthorized))
}

#[test]
fn acting_for_the_owner_she_never_shows_another_members_dm_with_her() {
    let w = world();
    let for_owner = w.acting_for(&w.owner);
    let all = ChatConversationsParams {
        include_archived: true,
    };
    // The owner's own list holds every DM in the organization, Bob's with her included.
    let owner_list = w
        .office
        .list_conversations(&w.owner, all.clone())
        .unwrap()
        .unwrap();
    assert!(owner_list.conversations.iter().any(|c| c.id == w.bob_dm));

    let listed = w.narrowed::<ChatConversations>(all, &for_owner, |office, tess, p| {
        office.list_conversations(tess, p).unwrap().unwrap()
    });
    let ids: Vec<&str> = result(&listed)["conversations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["id"].as_str().unwrap())
        .collect();
    assert!(ids.contains(&w.owner_dm.as_str()), "{ids:?}");
    assert!(
        !ids.contains(&w.bob_dm.as_str()),
        "Bob's DM leaked: {ids:?}"
    );

    let history = |conversation: &str| {
        w.narrowed::<ChatMessages>(
            ChatMessagesParams {
                conversation: Some(conversation.to_owned()),
                thread_id: None,
                limit: None,
                cursor: None,
            },
            &for_owner,
            |office, tess, p| office.chat_messages(tess, p).unwrap().unwrap(),
        )
    };
    assert_eq!(error_code(&history(&w.bob_dm)), Some(ErrorCode::NotFound));
    assert!(result(&history(&w.owner_dm))["messages"]
        .to_string()
        .contains("the owner's private plan"));

    let inbox = w.narrowed::<ChatInbox>(
        ChatInboxParams {
            include_all: true,
            ..Default::default()
        },
        &for_owner,
        |office, tess, p| office.chat_inbox(tess, p).unwrap().unwrap(),
    );
    let items = result(&inbox)["items"].as_array().unwrap().clone();
    let in_inbox = |conversation: &str| {
        items
            .iter()
            .any(|i| i["conversation_id"].as_str() == Some(conversation))
    };
    assert!(in_inbox(&w.owner_dm), "{items:?}");
    assert!(
        !in_inbox(&w.bob_dm),
        "Bob's DM leaked into her inbox: {items:?}"
    );
}

#[test]
fn dm_open_needs_a_caller_and_answers_only_a_dm_the_caller_is_in() {
    let w = world();
    let for_bob = w.acting_for(&w.bob);
    let open = |with: &str| DmOpenParams {
        participants: vec![with.to_owned()],
        name: None,
        conversation_id: None,
    };
    // Acting for nobody, she opens no DM at all.
    let wake = Delegation::default();
    assert!(refused(&w.admit(
        DmOpen::METHOD,
        serde_json::to_value(open(&w.bob)).unwrap(),
        &wake
    )));

    // Acting for Bob, his own DM with her opens; the owner's never does.
    assert!(w
        .admit(
            DmOpen::METHOD,
            serde_json::to_value(open(&w.owner)).unwrap(),
            &for_bob
        )
        .is_ok());
    let opened = |with: &str| {
        w.narrowed::<DmOpen>(open(with), &for_bob, |office, tess, p| {
            office.dm_open(tess, p).unwrap().unwrap()
        })
    };
    assert_eq!(
        error_code(&opened(&w.owner)),
        Some(ErrorCode::NotAuthorized),
        "the owner's DM with her never opens for Bob"
    );
    let own = opened(&w.bob);
    assert_eq!(result(&own)["conversation"]["id"], w.bob_dm.as_str());
}

#[test]
fn she_marks_reacts_answers_or_triages_only_in_a_conversation_the_caller_is_in() {
    let w = world();
    let for_bob = w.acting_for(&w.bob);
    let for_owner = w.acting_for(&w.owner);
    let wake = Delegation::default();
    let channel = w
        .office
        .list_conversations(
            &w.tess,
            ChatConversationsParams {
                include_archived: true,
            },
        )
        .unwrap()
        .unwrap()
        .conversations
        .into_iter()
        .find(|c| c.kind == org_proto::chat::ConversationKind::Channel)
        .map(|c| c.id);

    let on_conversation = |conversation: &str| {
        [
            ("chat.mark_read", json!({"conversation_id": conversation})),
            (
                "chat.mute",
                json!({"conversation_id": conversation, "muted_employee_id": w.tess, "mute_mode": "muted"}),
            ),
        ]
    };
    let on_message = |message: &str| {
        [
            (
                "chat.react",
                json!({"message_id": message, "reaction": "seen"}),
            ),
            ("chat.answer", json!({"ask_id": message, "answers": []})),
            ("msg.withdraw", json!({"ask_id": message})),
            ("msg.accept", json!({"id": message})),
            (
                "msg.defer",
                json!({"id": message, "until": {"type": "after_task"}}),
            ),
            ("msg.done", json!({"id": message})),
            ("msg.unsnooze", json!({"id": message})),
        ]
    };
    for (method, params) in on_conversation(&w.owner_dm)
        .into_iter()
        .chain(on_message(&w.owner_message))
    {
        assert!(
            refused(&w.admit(method, params.clone(), &for_bob)),
            "{method} on the owner's DM for Bob"
        );
        assert!(
            refused(&w.admit(method, params.clone(), &wake)),
            "{method} on a DM for nobody"
        );
        assert!(
            w.admit(method, params, &for_owner).is_ok(),
            "{method} on the owner's DM for the owner"
        );
    }
    for (method, params) in on_conversation(&w.bob_dm)
        .into_iter()
        .chain(on_message(&w.bob_message))
    {
        assert!(
            refused(&w.admit(method, params.clone(), &for_owner)),
            "{method} on Bob's DM for the owner"
        );
        assert!(
            w.admit(method, params, &for_bob).is_ok(),
            "{method} on Bob's DM for Bob"
        );
    }
    // A channel stays open to a wake; an unknown message is the method's to refuse.
    if let Some(channel) = channel {
        assert!(w
            .admit("chat.mark_read", json!({"conversation_id": channel}), &wake)
            .is_ok());
    }
    assert!(w
        .admit("msg.done", json!({"id": next_id()}), &for_bob)
        .is_ok());
}

#[test]
fn a_draft_keeps_the_member_it_was_first_made_for() {
    let w = world();
    w.gate.record_draft("req-1", &w.bob);
    w.gate.record_draft("req-1", &w.owner);
    assert_eq!(
        w.gate.draft_caller("req-1").as_deref(),
        Some(w.bob.as_str())
    );
    assert!(w.gate.is_chief_of_staff(&w.tess));
    assert!(!w.gate.is_chief_of_staff(&w.bob));
}
