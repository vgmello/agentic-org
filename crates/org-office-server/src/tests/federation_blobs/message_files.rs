//! `TASK-240` Arch H-1: the file of an HQ chat message, ask or answer (`TASK-98.2`, kept in
//! HQ's shared blob store and named by the message rather than a standalone record) is served
//! to a satellite on demand, over real Organization-CA mTLS: its record reaches the satellite
//! with the relayed message, an authorized reader there pulls the bytes from HQ through
//! `attachment_peer_fetch`, they are verified and cached, and the cache serves them once HQ is
//! gone. A private file is still never served, by any path.

use std::sync::Arc;
use std::time::Duration;

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use org_office::rules::OrgCommand;
use org_office::Office;
use org_proto::ask::{AskQuestion, AskQuestionType, ChatAnswerParams, ChatAskParams};
use org_proto::attachments::{
    AttachmentAttachParams, AttachmentTargetKind, AttachmentUploadBeginParams,
    AttachmentUploadChunkParams, AttachmentUploadRef,
};
use org_proto::chat::ChatSendParams;
use org_proto::org::MemberAddPersonParams;
use org_proto::ErrorCode;
use org_sync_proto::blob_digest;

use super::super::a2a_messaging::{hire_agent_at, setup_provider_on_hq, wait_until};
use super::{inspect, pull_org_events, read_rpc, Net};
use crate::federation::SatelliteSyncSession;
use org_office::attachments::remote::PullFailure;

/// Stages a committed text upload of `bytes` as `actor`: its upload ID.
fn staged(office: &Office, actor: &str, name: &str, bytes: &[u8]) -> String {
    let begun = office
        .attachment_upload_begin(
            actor,
            AttachmentUploadBeginParams {
                filename: name.into(),
                mime_type: "text/plain".into(),
                byte_count: bytes.len() as u64,
            },
        )
        .unwrap()
        .unwrap();
    let upload_id = begun.upload.upload_id;
    office
        .attachment_upload_chunk(
            actor,
            AttachmentUploadChunkParams {
                upload_id: upload_id.clone(),
                offset: 0,
                data_base64: STANDARD.encode(bytes),
            },
        )
        .unwrap()
        .unwrap();
    office
        .attachment_upload_commit(
            actor,
            AttachmentUploadRef {
                upload_id: upload_id.clone(),
            },
        )
        .unwrap()
        .unwrap();
    upload_id
}

fn add_person(hq: &Office, owner: &str, name: &str, handle: &str) -> String {
    let member = hq.next_uuid_v7();
    let team = hq.org_snapshot().unwrap().teams[0].id.clone();
    hq.submit_org(
        owner,
        OrgCommand::MemberAddPerson(MemberAddPersonParams {
            command_id: hq.next_uuid_v7(),
            member_id: member.clone(),
            name: name.into(),
            handle: Some(handle.into()),
            home_team_id: Some(team),
            contact: None,
        }),
    )
    .unwrap()
    .unwrap();
    member
}

/// At HQ, the owner asks `whom` and they answer with `bytes`: the answer's attachment ID.
fn answer_file(hq: &Office, owner: &str, whom: &str, bytes: &[u8]) -> String {
    let ask = hq
        .msg_ask(
            owner,
            ChatAskParams {
                id: hq.next_uuid_v7(),
                to: whom.into(),
                body: Some("Which draft?".into()),
                subject: None,
                questions: vec![AskQuestion {
                    id: "q1".into(),
                    question_type: AskQuestionType::FreeText,
                    prompt: "Which draft?".into(),
                    options: Vec::new(),
                    allow_multiple: false,
                    default_value: None,
                    optional: false,
                }],
                required: false,
                wait_ms: None,
                hold_ms: None,
                blocks: None,
                approval: false,
                addressed_to: None,
                meta: Default::default(),
                attachment_ids: Vec::new(),
            },
        )
        .unwrap()
        .unwrap();
    let upload = staged(hq, whom, "answer.txt", bytes);
    let answered = hq
        .chat_answer(
            whom,
            ChatAnswerParams {
                ask_id: ask.result.ask_id,
                answers: Vec::new(),
                option_id: None,
                text: Some("this one".into()),
                attachment_ids: vec![upload],
            },
        )
        .unwrap()
        .unwrap();
    answered.result.answer.attachments[0].id.clone()
}

#[test]
fn federation_serves_a_satellite_the_file_of_an_hq_message_or_answer_and_never_a_private_one() {
    let mut net = Net::new(1);
    let (hq, owner) = (Arc::clone(&net.hq), net.owner.clone());
    let sat = Arc::clone(net.sat(0));
    let team = hq.org_snapshot().unwrap().teams[0].id.clone();
    let (_, profile) = setup_provider_on_hq(&hq, &owner);
    let sat_id = sat.info().office_id.clone();
    let ben = hire_agent_at(&hq, &owner, &team, &sat_id, &profile, "Ben", "ben");
    pull_org_events(&sat);

    // At HQ the owner sends Ben, placed on the satellite, a file in their DM.
    let bytes = b"OWNER-TO-BEN: the launch brief, for Ben's eyes".to_vec();
    let upload = staged(&hq, &owner, "brief.txt", &bytes);
    let sent = hq
        .chat_send(
            &owner,
            ChatSendParams {
                id: hq.next_uuid_v7(),
                to: "ben".into(),
                kind: "msg".into(),
                priority: None,
                subject: None,
                body: "the brief".into(),
                attachment_ids: vec![upload],
                meta: Default::default(),
                reply_to: None,
                also_in_channel: false,
                reason: None,
            },
        )
        .unwrap()
        .unwrap();
    let file = sent.message.attachments[0].id.clone();
    // HQ keeps no standalone record of it, yet answers a satellite's fetch for it.
    let fetched = hq.attachment_peer_fetch(&file).unwrap().unwrap();
    assert_eq!(fetched.byte_count, bytes.len() as u64);
    crate::federation::sync_native_message_relays(&hq);

    // The relayed message brings the file's record to the satellite, never its bytes.
    let mut session = SatelliteSyncSession::connect(Arc::clone(&sat)).unwrap();
    wait_until(
        Duration::from_secs(30),
        "Ben's message reaches the satellite",
        || {
            let _ = session.receive_inbound_relays(None);
            inspect(&sat, &ben, &file).is_ok()
        },
    );
    drop(session);
    let view = inspect(&sat, &ben, &file).unwrap();
    assert_eq!(view.target_kind, AttachmentTargetKind::Message);
    let cache = sat.attachment_blob_cache();
    assert_eq!(cache.count().unwrap(), 0);
    assert_eq!(sat.attachment_blob_counts().unwrap(), (0, 0));

    // Ben reads it on the satellite: pulled from HQ, verified, cached.
    assert_eq!(read_rpc(&sat, &ben, &file).unwrap(), bytes);
    assert_eq!(blob_digest(&bytes), view.content_sha256);
    assert_eq!(cache.count().unwrap(), 1);
    // The owner's answer file, too, is HQ's to serve by ID alone.
    let ada = add_person(&hq, &owner, "Ada", "ada");
    let answer = answer_file(&hq, &owner, &ada, b"ADA-ANSWER: the second draft");
    let mut session = SatelliteSyncSession::connect(Arc::clone(&sat)).unwrap();
    let blob = hq.attachment_peer_fetch(&answer).unwrap().unwrap();
    let pulled = session
        .fetch_shared_blob(&answer, blob.content_sha256.as_str(), blob.byte_count)
        .unwrap();
    assert_eq!(pulled, b"ADA-ANSWER: the second draft");

    // A private file at HQ stays out of reach by its ID or its digest.
    let secret = staged(&hq, &owner, "secret.txt", b"PRIVATE-owner-only");
    let private = hq
        .attachment_attach(
            &owner,
            AttachmentAttachParams {
                command_id: hq.next_uuid_v7(),
                upload_id: Some(secret),
                source_attachment_id: None,
                target_kind: AttachmentTargetKind::UserPrivate,
                target_id: None,
                title: None,
                description: None,
                artifact_status: None,
            },
        )
        .unwrap()
        .unwrap()
        .attachment;
    let refused = hq.attachment_peer_fetch(&private.id).unwrap().unwrap_err();
    assert_eq!(refused.code, ErrorCode::NotFound);
    for asked in [&private.id, &private.content_sha256] {
        match session.fetch_shared_blob(asked, &private.content_sha256, private.byte_count) {
            Err(PullFailure::Refused { code, .. }) => assert_eq!(code, "not_found", "{asked}"),
            other => panic!("{asked}: {other:?}"),
        }
    }
    drop(session);

    // HQ gone: Ben still reads his file from the satellite's cache.
    net.stop_hq();
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(read_rpc(&sat, &ben, &file).unwrap(), bytes);
}
