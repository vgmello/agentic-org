//! Wake classification of committed sends (`TASK-80.1`, pass 4): an `@mention` inside a DM is
//! a direct message, never a mention, so the Chief of Staff runs the owner's private DM in the
//! owner's own partition and never in her shared `tess:wake`.

use org_office::{Office, TestOffice};
use org_office_sessions::WakeKind;
use org_proto::chat::{ChatSendParams, ConversationKind, DeliveredBecause, DmOpenParams};

use super::wake_kind_for_send;

#[test]
fn an_owner_dm_mentioning_tess_wakes_her_as_a_direct_message() {
    let office = Office::for_test(TestOffice::default());
    let owner = office.owner_member_id().to_owned();
    let tess = office.chief_of_staff().unwrap().expect("Tess");
    let tess_id = tess.id.to_string();
    office
        .dm_open(
            &owner,
            DmOpenParams {
                participants: vec![tess_id.clone()],
                name: None,
                conversation_id: None,
            },
        )
        .unwrap()
        .unwrap();
    let committed = office
        .chat_send(
            &owner,
            ChatSendParams {
                id: "0190c6a0-0000-7000-8000-00000000d1a1".into(),
                to: tess_id.clone(),
                kind: "msg".into(),
                priority: None,
                subject: None,
                body: format!(
                    "@{} confidential: the acquisition closes Friday",
                    tess.handle
                ),
                attachment_ids: Vec::new(),
                meta: Default::default(),
                reply_to: None,
                also_in_channel: false,
                reason: None,
            },
        )
        .unwrap()
        .unwrap();
    assert_eq!(committed.conversation_kind, ConversationKind::Dm);
    let (_, state) = committed
        .recipients
        .iter()
        .find(|(id, _)| id.as_str() == tess_id)
        .expect("Tess receives it");
    let mentioned = committed
        .result
        .mentions
        .iter()
        .any(|m| m.employee_id == tess_id);
    assert_eq!(
        wake_kind_for_send(&committed, &tess_id, state.delivered_because, mentioned),
        Some(WakeKind::DirectMessage)
    );
    // However the delivery is labelled, a DM never wakes her as a mention.
    for because in [DeliveredBecause::Mention, DeliveredBecause::Addressed] {
        assert_eq!(
            wake_kind_for_send(&committed, &tess_id, because, true),
            Some(WakeKind::DirectMessage),
            "{because:?}"
        );
    }
}
