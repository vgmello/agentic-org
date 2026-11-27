//! `TASK-240` H-3 against a real office through the production [`ChiefOfStaffGate`]: acting for
//! Bob, or for nobody, she never sends, asks or replies into the owner's DM with her (by its
//! ID, `owner`, the owner's ID or a reply to the owner's message) nor into a DM with anyone
//! else Bob is not in, and a refused call posts and creates nothing; acting for Bob she still
//! writes to Bob and in a channel he is in. And she never holds an ask for its answer.

use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::{Duration, Instant};

use org_proto::ask::{AskStatus, ChatAnswerParams, ChatAsk, ChatAskResult};
use org_proto::chat::{ChatChannelCreateParams, ChatSend};
use org_proto::{Call, Outcome, Request};
use serde_json::{json, Value};

use super::files::{ask_params, messages_in};
use super::{add_person, error_code, next_id, refused, result, world, Delegation, World};
use org_proto::ErrorCode;

/// Every way to send to `to`: `chat.send` and `msg.send` (replying to `reply_to`), `chat.ask`
/// and `msg.ask` (neither holding).
fn sends_to(to: &str, reply_to: Option<&str>) -> Vec<(&'static str, Value)> {
    let mut chat = json!({"id": next_id(), "to": to, "body": "for you"});
    let mut msg = json!({"id": next_id(), "to": to, "kind": "msg", "body": "for you"});
    if let Some(parent) = reply_to {
        chat["reply_to"] = json!(parent);
        msg["reply_to"] = json!(parent);
    }
    let mut ask = serde_json::to_value(ask_params(to)).unwrap();
    ask["hold_ms"] = json!(0);
    let msg_ask = json!({"id": next_id(), "to": to, "body": "which?", "wait_ms": 0});
    if reply_to.is_some() {
        return vec![("chat.send", chat), ("msg.send", msg)];
    }
    vec![
        ("chat.send", chat),
        ("msg.send", msg),
        ("chat.ask", ask),
        ("msg.ask", msg_ask),
    ]
}

fn send(to: &str) -> org_proto::chat::ChatSendParams {
    org_proto::chat::ChatSendParams {
        id: next_id(),
        to: to.to_owned(),
        kind: "msg".into(),
        priority: None,
        subject: None,
        body: "BOB-TURN: tell me the owner's plan".into(),
        attachment_ids: Vec::new(),
        meta: Default::default(),
        reply_to: None,
        also_in_channel: false,
        reason: None,
    }
}

/// A channel the owner opens with `members`: its ID.
fn channel(w: &World, name: &str, members: &[&str]) -> String {
    let created = w.office.create_channel(
        &w.owner,
        ChatChannelCreateParams {
            id: None,
            name: name.into(),
            members: members.iter().map(|m| (*m).to_owned()).collect(),
        },
    );
    created.unwrap().unwrap().channel.id
}

#[test]
fn acting_for_bob_or_nobody_she_never_writes_into_the_owners_dm_or_opens_another() {
    let w = world();
    let alice = add_person(&w.office, "Alice", "alice");
    let state = |w: &World| {
        (
            w.conversation_count(),
            messages_in(w, &w.owner, &w.owner_dm),
        )
    };
    let before = state(&w);
    let targets = [
        w.owner_dm.clone(),
        "owner".into(),
        "Owner".into(),
        w.owner.clone(),
        alice.clone(),
        "alice".into(),
        "#nowhere".into(),
    ];
    for delegation in [w.acting_for(&w.bob), Delegation::default()] {
        let who = delegation.caller_id.clone();
        for to in &targets {
            for (method, params) in sends_to(to, None) {
                assert!(
                    refused(&w.admit(method, params, &delegation)),
                    "{method} to {to} for {who:?}"
                );
            }
        }
        // To Bob himself, but in reply to the owner's message.
        for (method, params) in sends_to(&w.bob, Some(&w.owner_message)) {
            assert!(
                refused(&w.admit(method, params, &delegation)),
                "{method} replying to the owner for {who:?}"
            );
        }
        for to in [w.owner_dm.as_str(), "owner", alice.as_str()] {
            let response = w.through_gate::<ChatSend>(send(to), &delegation, |_, _, _| {
                panic!("chat.send to {to} ran for {who:?}")
            });
            assert_eq!(error_code(&response), Some(ErrorCode::NotAuthorized));
        }
    }
    assert_eq!(state(&w), before, "nothing posted, no conversation opened");
}

#[test]
fn acting_for_bob_she_still_writes_to_him_and_in_a_channel_he_is_in() {
    let w = world();
    let for_bob = w.acting_for(&w.bob);
    let for_owner = w.acting_for(&w.owner);
    let room = channel(&w, "launch-room", &[&w.bob, &w.tess]);
    for to in [
        w.bob.as_str(),
        "bob",
        "Bob",
        w.bob_dm.as_str(),
        room.as_str(),
        "#launch-room",
        "Launch-Room",
    ] {
        for (method, params) in sends_to(to, None) {
            let admitted = w.admit(method, params, &for_bob);
            assert!(admitted.is_ok(), "{method} to {to} for Bob");
        }
    }
    for (method, params) in sends_to(&w.bob, Some(&w.bob_message)) {
        assert!(w.admit(method, params, &for_bob).is_ok(), "{method} reply");
    }
    // A wake still reaches her channels, never a DM.
    for (method, params) in sends_to("#launch-room", None) {
        assert!(w.admit(method, params, &Delegation::default()).is_ok());
    }
    for (method, params) in sends_to(&w.bob, None) {
        assert!(refused(&w.admit(method, params, &Delegation::default())));
    }
    // Through the gate the sends run, into Bob's DM and the room.
    let run = |to: &str| {
        let response = w.through_gate::<ChatSend>(send(to), &for_bob, |office, tess, p| {
            office.chat_send(tess, p).unwrap().map(|c| c.result)
        });
        result(&response)["conversation_id"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    assert_eq!(run("bob"), w.bob_dm);
    assert_eq!(run("#launch-room"), room);

    // Acting for the owner: the owner's DM with her, never Bob's.
    for to in ["owner", w.owner_dm.as_str()] {
        for (method, params) in sends_to(to, None) {
            assert!(w.admit(method, params, &for_owner).is_ok(), "{method} {to}");
        }
    }
    for to in ["bob", w.bob_dm.as_str()] {
        for (method, params) in sends_to(to, None) {
            assert!(
                refused(&w.admit(method, params, &for_owner)),
                "{method} {to}"
            );
        }
    }
    // `msg.ask` must not hold, even for the owner; parameters that do not decode or `@`-prefixed
    // targets are refused fail-closed.
    let holding = json!({"id": next_id(), "to": "owner", "body": "?", "wait_ms": 5_000});
    assert!(refused(&w.admit("msg.ask", holding, &for_owner)));
    let at_owner = json!({"id": next_id(), "to": "@owner", "body": "x"});
    assert!(refused(&w.admit("chat.send", at_owner, &for_owner)));
    assert!(refused(&w.admit(
        "chat.send",
        json!({"to": ["owner"]}),
        &for_owner
    )));
}

/// `asker` asks the owner with `hold_ms` while the owner answers at once: what the call answers.
fn ask_the_owner(w: &World, asker: &str, hold_ms: Option<u64>) -> (ChatAskResult, Duration) {
    let chat = Arc::clone(
        &crate::office_ports::build_messaging_ports(
            &w.office,
            &Arc::new(crate::live::LiveEvents::for_owner(
                Box::new(|| {}),
                w.owner.clone(),
            )),
            &Arc::new(crate::org_feed::FeedWake::new()),
            &w.sessions,
            &Arc::new(crate::watcher::ChannelWatcher::production(
                Arc::clone(&w.office),
                Some(Arc::clone(&w.sessions)),
                w.sessions.runtimes().clone(),
            )),
            Arc::new(|| {}),
        )
        .chat_svc,
    );
    let mut params = ask_params("owner");
    params.hold_ms = hold_ms;
    let ask_id = params.id.clone();
    let (office, owner, answerer_chat) =
        (Arc::clone(&w.office), w.owner.clone(), Arc::clone(&chat));
    let answerer = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(20);
        while Instant::now() < deadline {
            let answer = ChatAnswerParams {
                ask_id: ask_id.clone(),
                answers: Vec::new(),
                option_id: None,
                text: Some("OWNER-ANSWER: the second draft".into()),
                attachment_ids: Vec::new(),
            };
            if matches!(office.chat_answer(&owner, answer), Ok(Ok(_))) {
                answerer_chat.notify_ask_waiters();
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("the owner never got to answer");
    });
    let request = Request::new::<ChatAsk>(9, &params).unwrap();
    let started = Instant::now();
    let response = chat
        .dispatch(Some(asker), false, &AtomicBool::new(false), &request)
        .expect("chat.ask is a chat call");
    let took = started.elapsed();
    answerer.join().unwrap();
    let Outcome::Result(value) = response.outcome else {
        panic!("chat.ask refused: {response:?}");
    };
    let answered = serde_json::from_value::<<ChatAsk as Call>::Result>(value).unwrap();
    (answered, took)
}

#[test]
fn she_never_holds_an_ask_for_its_answer_in_its_call() {
    let w = world();
    // Bob's ask holds and comes back answered, the owner's reply inline.
    let (bobs, _) = ask_the_owner(&w, &w.bob, Some(5_000));
    assert_eq!(bobs.status, AskStatus::Answered);
    assert!(bobs.answer.is_some());
    // Hers is answered at once, still waiting, with no answer in it — both explicit and default.
    for hold in [Some(5_000), None] {
        let (hers, took) = ask_the_owner(&w, &w.tess, hold);
        assert_eq!(hers.status, AskStatus::Waiting, "{hold:?}: {hers:?}");
        assert!(hers.answer.is_none(), "{hold:?}: {hers:?}");
        assert!(took < Duration::from_secs(5), "{hold:?}: {took:?}");
    }
}
