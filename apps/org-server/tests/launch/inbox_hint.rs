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
