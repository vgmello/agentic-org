//! Wake classification of committed sends (`TASK-80.1`, pass 4): an `@mention` inside a DM is
//! a direct message, never a mention, so the Chief of Staff runs the owner's private DM in the
//! owner's own partition and never in her shared `tess:wake`.

use org_office::{Office, TestOffice};
use org_office_sessions::WakeKind;
use org_proto::attachments::{ArtifactStatus, AttachmentMeta};
use org_proto::chat::{ChatSendParams, ConversationKind, DeliveredBecause, DmOpenParams};
use org_proto::tasks::{TaskCommentWire, TaskPriority, TaskStatus, TaskView, TasksCommentResult};

use super::{comment_wake_summary, wake_kind_for_send};

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

#[test]
fn task_comment_wake_summary_carries_lightweight_attachment_ids_only() {
    let att_id = "0193a5c0-0000-7000-8000-00000000a101";
    let result = TasksCommentResult {
        task: TaskView {
            id: "0193a5c0-0000-7000-8000-000000000100".into(),
            office_code: "A".into(),
            number: 1,
            key: "T-A1".into(),
            revision: 2,
            team_id: "0193a5c0-0000-7000-8000-000000000010".into(),
            assigned_team_id: None,
            assignee_id: None,
            project_id: None,
            task_type: "task".into(),
            work_type: None,
            title: "Attach".into(),
            status: TaskStatus::Todo,
            body_markdown: String::new(),
            feature_id: None,
            milestone_id: None,
            priority: TaskPriority::Normal,
            effort: None,
            severity: None,
            rank_order: None,
            labels: vec![],
            reference_files: vec![],
            modified_files: vec![],
            depends_on: vec![],
            dependents: vec![],
            blocked_reason: None,
            blocked_since: None,
            waiting_on_owner: false,
            dependency_blocked: false,
            unmet_blocking_dependencies: vec![],
            resolution: None,
            created_by_kind: "employee".into(),
            created_by_id: None,
            updated_by_kind: "employee".into(),
            updated_by_id: None,
            created_at: None,
            updated_at: None,
            checklist_total: 0,
            checklist_done: 0,
            ac_total: 0,
            ac_done: 0,
            dod_total: 0,
            dod_done: 0,
            work_steps_total: 0,
            work_steps_done: 0,
            comment_count: 1,
            watcher_count: 0,
            details: None,
            final_summary: None,
        },
        comment: TaskCommentWire {
            id: "0193a5c0-0000-7000-8000-000000000300".into(),
            position: 0,
            author_id: "0193a5c0-0000-7000-8000-000000000020".into(),
            created_at: "2026-11-27T00:00:00.000Z".into(),
            body_markdown: String::new(),
            attachments: vec![AttachmentMeta {
                id: att_id.into(),
                filename: "report.pdf".into(),
                title: "Report".into(),
                description: None,
                artifact_status: ArtifactStatus::Final,
                mime_type: "application/pdf".into(),
                byte_count: 512,
                width: None,
                height: None,
                page_count: Some(2),
                uploaded_by_employee_id: "0193a5c0-0000-7000-8000-000000000020".into(),
                created_at: "2026-11-27T00:00:00.000Z".into(),
            }],
        },
    };
    let summary = comment_wake_summary(&result);
    assert!(summary.contains("T-A1"), "{summary}");
    assert!(summary.contains(att_id), "{summary}");
    assert!(summary.contains("[attachment_ids:"), "{summary}");
    assert!(!summary.contains("/tmp"), "{summary}");
}

#[test]
fn waking_dm_ask_and_ask_answer_thread_lightweight_attachment_ids_without_base64() {
    use std::sync::Arc;

    use base64::engine::general_purpose::STANDARD;
    use base64::Engine as _;
    use org_office_sessions::{SessionManager, SessionManagerConfig};
    use org_proto::ask::{
        AskQuestion, AskQuestionType, ChatAnswerParams, ChatAskParams, QuestionAnswer,
    };
    use org_proto::attachments::{
        AttachmentUploadBeginParams, AttachmentUploadChunkParams, AttachmentUploadRef,
    };

    const PNG: &[u8] = &[
        137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 2, 0, 0, 0, 2, 8, 2,
        0, 0, 0, 253, 212, 154, 115, 0, 0, 0, 18, 73, 68, 65, 84, 120, 156, 99, 248, 207, 192, 192,
        0, 194, 12, 255, 129, 0, 0, 31, 238, 5, 251, 11, 217, 104, 139, 0, 0, 0, 0, 73, 69, 78, 68,
        174, 66, 96, 130,
    ];

    let upload = |office: &Office, actor: &str, name: &str, mime: &str, bytes: &[u8]| -> String {
        let begun = office
            .attachment_upload_begin(
                actor,
                AttachmentUploadBeginParams {
                    filename: name.into(),
                    mime_type: mime.into(),
                    byte_count: bytes.len() as u64,
                },
            )
            .unwrap()
            .unwrap();
        let uid = begun.upload.upload_id;
        office
            .attachment_upload_chunk(
                actor,
                AttachmentUploadChunkParams {
                    upload_id: uid.clone(),
                    offset: 0,
                    data_base64: STANDARD.encode(bytes),
                },
            )
            .unwrap()
            .unwrap();
        office
            .attachment_upload_commit(actor, AttachmentUploadRef { upload_id: uid })
            .unwrap()
            .unwrap()
            .upload
            .upload_id
    };

    let tmp = tempfile::tempdir().unwrap();
    let office = Arc::new(Office::for_test(TestOffice::default()));
    let owner = office.owner_member_id().to_owned();
    let hr = office
        .list_employees()
        .unwrap()
        .into_iter()
        .find(|e| e.builtin_key == Some(org_office::organization_family::records::BuiltinKey::Hr))
        .expect("HR agent");
    let hr_id = hr.id.to_string();

    let cfg = SessionManagerConfig::with_runtimes(
        tmp.path().join("office.sock"),
        org_office_sessions::RuntimeRegistry::empty(),
    );
    let sessions = SessionManager::new(Arc::clone(&office), cfg);
    // Hold launches so wakes stay in `pending-wakes.v1.json` for inspection.
    sessions.set_budget_hold(Some("inspect".into()));

    office
        .dm_open(
            &owner,
            DmOpenParams {
                participants: vec![hr_id.clone()],
                name: None,
                conversation_id: None,
            },
        )
        .unwrap()
        .unwrap();

    // 1. Waking DM with PNG + TXT attachments.
    let u_dm_img = upload(&office, &owner, "dm.png", "image/png", PNG);
    let u_dm_doc = upload(&office, &owner, "dm.txt", "text/plain", b"hello doc");
    let dm_sent = office
        .chat_send(
            &owner,
            ChatSendParams {
                id: "0190c6a0-0000-7000-8000-00000000d1a2".into(),
                to: hr_id.clone(),
                kind: "msg".into(),
                priority: None,
                subject: None,
                body: "See attached".into(),
                attachment_ids: vec![u_dm_img, u_dm_doc],
                meta: Default::default(),
                reply_to: None,
                also_in_channel: false,
                reason: None,
            },
        )
        .unwrap()
        .unwrap();
    let dm_att_ids: Vec<String> = dm_sent
        .message
        .attachments
        .iter()
        .map(|a| a.id.clone())
        .collect();
    assert_eq!(dm_att_ids.len(), 2);
    let _ = super::on_chat_send_committed(&office, &sessions, &owner, &dm_sent);

    // 2. Waking Ask with PNG attachment.
    let u_ask = upload(&office, &owner, "ask.png", "image/png", PNG);
    let ask_sent = office
        .msg_ask(
            &owner,
            ChatAskParams {
                id: "0190c6a0-0000-7000-8000-00000000d1a3".into(),
                to: hr_id.clone(),
                subject: None,
                required: false,
                approval: false,
                addressed_to: Some(hr_id.clone()),
                wait_ms: None,
                hold_ms: None,
                blocks: None,
                body: Some("Approve this?".into()),
                attachment_ids: vec![u_ask],
                questions: vec![AskQuestion {
                    id: "q1".into(),
                    question_type: AskQuestionType::FreeText,
                    prompt: "Thoughts?".into(),
                    options: Vec::new(),
                    allow_multiple: false,
                    default_value: None,
                    optional: false,
                }],
                meta: Default::default(),
            },
        )
        .unwrap()
        .unwrap();
    let ask_att_ids: Vec<String> = ask_sent
        .message
        .attachments
        .iter()
        .map(|a| a.id.clone())
        .collect();
    assert_eq!(ask_att_ids.len(), 1);
    let _ = super::on_chat_ask_committed(&office, &sessions, &owner, &ask_sent);

    // 3. Ask from HR answered by owner with attachment -> wakes HR with answer attachment ID!
    let hr_ask = office
        .msg_ask(
            &hr_id,
            ChatAskParams {
                id: "0190c6a0-0000-7000-8000-00000000d1a4".into(),
                to: owner.clone(),
                subject: None,
                required: false,
                approval: false,
                addressed_to: Some(owner.clone()),
                wait_ms: None,
                hold_ms: None,
                blocks: None,
                body: Some("Need spec".into()),
                attachment_ids: Vec::new(),
                questions: vec![AskQuestion {
                    id: "q1".into(),
                    question_type: AskQuestionType::FreeText,
                    prompt: "Which file?".into(),
                    options: Vec::new(),
                    allow_multiple: false,
                    default_value: None,
                    optional: false,
                }],
                meta: Default::default(),
            },
        )
        .unwrap()
        .unwrap();
    let u_ans = upload(&office, &owner, "answer.txt", "text/plain", b"approved");
    let ans = office
        .chat_answer(
            &owner,
            ChatAnswerParams {
                ask_id: hr_ask.result.ask_id.clone(),
                answers: vec![QuestionAnswer {
                    question_id: "q1".into(),
                    option_ids: Vec::new(),
                    text: Some("Use attached".into()),
                }],
                option_id: None,
                text: None,
                attachment_ids: vec![u_ans],
            },
        )
        .unwrap()
        .unwrap();
    let ans_att_ids: Vec<String> = ans
        .result
        .answer
        .attachments
        .iter()
        .map(|a| a.id.clone())
        .collect();
    assert_eq!(ans_att_ids.len(), 1);
    super::on_chat_answer_committed(&office, &sessions, &owner, &ans, false);

    // Verify all 3 persisted wake items hold exact lightweight IDs and ZERO base64!
    let q = sessions
        .store()
        .read_pending_wakes(&hr_id)
        .unwrap()
        .expect("pending wakes persisted");
    assert_eq!(q.items.len(), 3);
    assert_eq!(q.items[0].attachment_ids, dm_att_ids);
    assert_eq!(q.items[1].attachment_ids, ask_att_ids);
    assert_eq!(q.items[2].attachment_ids, ans_att_ids);
    let raw =
        std::fs::read_to_string(sessions.store().pending_wakes_path(&hr_id).unwrap()).unwrap();
    let png_b64_prefix = &STANDARD.encode(PNG)[..16];
    assert!(
        !raw.contains(png_b64_prefix),
        "zero base64 in durable wake JSON"
    );
}
