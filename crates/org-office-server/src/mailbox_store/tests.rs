//! The owner Inbox over a real office on both engines (`TASK-98.2`): items carry their
//! message's files, `has:image` / `has:file` filter them (counts never), and the inline Reply
//! and Deny actions take files, as calls the office accepts unchanged.

use super::*;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use org_office::{EngineChoice, TestOffice};
use org_proto::ask::{AskOption, AskQuestion, AskQuestionType, ChatAnswerParams, ChatAskParams};
use org_proto::attachments::{
    AttachmentUploadBeginParams, AttachmentUploadChunkParams, AttachmentUploadRef,
};
use org_proto::chat::{ChatMessagesParams, ChatSendParams};
use org_proto::inbox::{ActionInput, InboxListParams, InboxListResult};
use org_proto::msg::MsgReplyParams;
use serde_json::{json, Map, Value};
use std::sync::atomic::{AtomicU64, Ordering};

/// A 1x1 PNG.
const PNG: &str =
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNkYAAAAAYAAjCB0C8AAAAASUVORK5CYII=";

static NEXT: AtomicU64 = AtomicU64::new(1);

fn uuid() -> String {
    let n = NEXT.fetch_add(1, Ordering::Relaxed) + 9_000;
    let ms = org_office::organization_family::testkit::CREATION_AT.0 + n;
    let mut b = [0u8; 16];
    b[0..6].copy_from_slice(&ms.to_be_bytes()[2..8]);
    b[6] = 0x70 | ((n >> 8) as u8 & 0x0f);
    b[7] = (n & 0xff) as u8;
    b[8] = 0x80;
    b[15] = (n & 0xff) as u8;
    let hex: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

fn upload(office: &Office, member: &str, filename: &str, mime: &str, bytes: &[u8]) -> String {
    let begun = office
        .attachment_upload_begin(
            member,
            AttachmentUploadBeginParams {
                filename: filename.into(),
                mime_type: mime.into(),
                byte_count: bytes.len() as u64,
            },
        )
        .unwrap()
        .unwrap();
    let id = begun.upload.upload_id;
    office
        .attachment_upload_chunk(
            member,
            AttachmentUploadChunkParams {
                upload_id: id.clone(),
                offset: 0,
                data_base64: STANDARD.encode(bytes),
            },
        )
        .unwrap()
        .unwrap();
    office
        .attachment_upload_commit(
            member,
            AttachmentUploadRef {
                upload_id: id.clone(),
            },
        )
        .unwrap()
        .unwrap();
    id
}

fn png(office: &Office, member: &str) -> String {
    upload(
        office,
        member,
        "shot.png",
        "image/png",
        &STANDARD.decode(PNG).unwrap(),
    )
}

fn hr(office: &Office) -> String {
    let snapshot = office.org_snapshot().unwrap();
    let hr = snapshot
        .members
        .iter()
        .find(|m| m.handle.as_deref() == Some("hr"));
    hr.expect("seeded hr agent").id.clone()
}

fn list(hq: &crate::inbox::Hq, has_image: Option<bool>, has_file: Option<bool>) -> InboxListResult {
    hq.list(InboxListParams {
        state: InboxState::Open,
        kinds: None,
        project: None,
        team: None,
        cursor: None,
        limit: Some(100),
        has_image,
        has_file,
    })
    .unwrap()
}

fn refs(page: &InboxListResult, source: &[&str]) -> Vec<String> {
    let mut refs: Vec<String> = page
        .items
        .iter()
        .filter(|i| source.contains(&i.source.as_str()))
        .map(|i| i.reference.clone())
        .collect();
    refs.sort();
    refs
}

/// The one `call` action labelled `label`: its op, params and input.
fn call_action<'a>(
    item: &'a InboxItem,
    label: &str,
) -> (&'a str, &'a Map<String, Value>, Option<&'a ActionInput>) {
    item.actions
        .iter()
        .find_map(|action| match action {
            Action::Call {
                label: l,
                op,
                params,
                input,
                ..
            } if l == label => Some((op.as_str(), params, input.as_ref())),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no {label} action: {:?}", item.actions))
}

fn takes_files(input: &ActionInput) -> bool {
    input.attachments_field.as_deref() == Some("attachment_ids") && input.max_attachments == 10
}

/// What a client sends for an inline action: its params, a fresh id where asked, the typed
/// input and files.
fn filled<T: serde::de::DeserializeOwned>(params: &Map<String, Value>, extra: Value) -> T {
    let mut all = params.clone();
    all.extend(extra.as_object().unwrap().clone());
    serde_json::from_value(Value::Object(all)).unwrap()
}

fn ask(office: &Office, hr: &str, approval: bool) -> String {
    let question = if approval {
        AskQuestion {
            id: "decision".into(),
            question_type: AskQuestionType::Choice,
            prompt: "Approve the release?".into(),
            options: ["approve", "reject"]
                .map(|id| AskOption {
                    id: id.into(),
                    label: id.into(),
                    description: None,
                })
                .to_vec(),
            allow_multiple: false,
            default_value: None,
            optional: false,
        }
    } else {
        AskQuestion {
            id: "q".into(),
            question_type: AskQuestionType::FreeText,
            prompt: "Which logo?".into(),
            options: Vec::new(),
            allow_multiple: false,
            default_value: None,
            optional: false,
        }
    };
    let params = ChatAskParams {
        id: uuid(),
        to: "owner".into(),
        body: None,
        subject: None,
        questions: vec![question],
        required: false,
        wait_ms: None,
        hold_ms: None,
        blocks: None,
        approval,
        addressed_to: None,
        meta: Default::default(),
        attachment_ids: Vec::new(),
    };
    office.msg_ask(hr, params).unwrap().unwrap().result.ask_id
}

#[test]
fn owner_inbox_items_carry_files_filter_by_has_and_answer_inline_with_files() {
    org_office_sqlite::install();
    for engine in [EngineChoice::Memory, EngineChoice::Sqlite] {
        let office = Arc::new(Office::for_test(TestOffice { engine }));
        let owner = office.owner_member_id().to_owned();
        let hr = hr(&office);
        let hq = crate::office_runtime::hq_inbox(Arc::clone(&office)).unwrap();
        let files = vec![
            png(&office, &hr),
            upload(&office, &hr, "brief.md", "text/markdown", b"# Brief\n"),
        ];
        let sent = office
            .chat_send(
                &hr,
                ChatSendParams {
                    id: uuid(),
                    to: "owner".into(),
                    kind: org_proto::msg::KIND_MSG.into(),
                    priority: None,
                    subject: Some("Mock-ups".into()),
                    body: String::new(),
                    meta: Default::default(),
                    reply_to: None,
                    also_in_channel: false,
                    reason: None,
                    attachment_ids: files,
                },
            )
            .unwrap()
            .unwrap();
        let approval = ask(&office, &hr, true);
        let question = ask(&office, &hr, false);

        let all = list(&hq, None, None);
        let message_ref = format!("{}:msg:{}", office.info().office_id, sent.result.id);
        let item = all
            .items
            .iter()
            .find(|i| i.reference == message_ref)
            .unwrap();
        assert_eq!(item.attachments.len(), 2, "{engine:?}");
        assert_eq!(item.attachments[0].mime_type, "image/png");
        assert_eq!(item.attachments[1].filename, "brief.md");
        // Filters shape the page, never the counts.
        let mine = ["mailbox", "approval", "ask"];
        let images = list(&hq, Some(true), None);
        assert_eq!(refs(&images, &mine), vec![message_ref.clone()]);
        assert_eq!(images.counts, all.counts);
        assert_eq!(
            refs(&list(&hq, None, Some(true)), &mine),
            vec![message_ref.clone()]
        );
        let plain = refs(&list(&hq, Some(false), Some(false)), &mine);
        assert_eq!(plain.len(), 2);
        assert!(!plain.contains(&message_ref));

        // Reply inline with a file: the action's params are a valid `msg.reply`.
        let (op, params, input) = call_action(item, "Reply");
        assert_eq!(op, "msg.reply");
        let input = input.unwrap();
        assert!(takes_files(input) && !input.required, "{input:?}");
        let reply: MsgReplyParams = filled(
            params,
            json!({"id": uuid(), "body": "", "attachment_ids": [png(&office, &owner)]}),
        );
        office.chat_reply(&owner, reply).unwrap().unwrap();
        let seen = office
            .chat_messages(
                &hr,
                ChatMessagesParams {
                    conversation: Some(sent.result.conversation_id.clone()),
                    has_image: Some(true),
                    ..Default::default()
                },
            )
            .unwrap()
            .unwrap();
        assert_eq!(seen.messages.len(), 2, "{engine:?}");

        // Deny an approval inline, saying why with a file.
        let approval_item = all
            .items
            .iter()
            .find(|i| i.reference.ends_with(&approval))
            .unwrap();
        assert_eq!(approval_item.kind, InboxKind::Approval);
        let (op, params, input) = call_action(approval_item, "Approve");
        assert_eq!(
            (op, params["option_id"].as_str()),
            ("chat.answer", Some("approve"))
        );
        assert!(input.is_none());
        let (op, params, input) = call_action(approval_item, "Deny");
        assert_eq!(
            (op, params["option_id"].as_str()),
            ("chat.answer", Some("reject"))
        );
        assert!(takes_files(input.unwrap()));
        let deny: ChatAnswerParams = filled(
            params,
            json!({"text": "Not this week", "attachment_ids": [png(&office, &owner)]}),
        );
        let answered = office.chat_answer(&owner, deny).unwrap().unwrap();
        assert_eq!(answered.result.answer.attachments.len(), 1);
        assert_eq!(
            answered.result.answer.text.as_deref(),
            Some("Not this week")
        );

        // A one-question free-text ask is answered inline too; its text is required.
        let question_item = all
            .items
            .iter()
            .find(|i| i.reference.ends_with(&question))
            .unwrap();
        let (op, params, input) = call_action(question_item, "Reply");
        assert_eq!(op, "chat.answer");
        assert!(input.unwrap().required && takes_files(input.unwrap()));
        let reply: ChatAnswerParams = filled(params, json!({"text": "The round one"}));
        office.chat_answer(&owner, reply).unwrap().unwrap();

        let after = list(&hq, None, None);
        for answered in [&approval, &question] {
            assert!(!after
                .items
                .iter()
                .any(|i| i.reference.ends_with(answered.as_str())));
        }
    }
}
