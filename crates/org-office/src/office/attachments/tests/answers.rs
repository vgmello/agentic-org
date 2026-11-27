//! Files on asks and ask answers on a real office, on both engines (`TASK-98.2`): an answer's
//! files persist on the participant's answer file and on the reply message that carries it,
//! and every message and answer file survives deleting and rebuilding the projection.

use org_proto::ask::{AskQuestion, AskQuestionType, ChatAnswerParams, ChatAskParams};
use org_proto::attachments::AttachmentMeta;
use org_proto::chat::ChatMessageView;
use org_proto::msg::KIND_REPLY;
use org_proto::ErrorCode;

use super::messages::{inspect, messages, pdf, png, send, still_staged};
use super::*;

fn ask_params(to: &str, files: Vec<String>) -> ChatAskParams {
    ChatAskParams {
        id: next_uuid(),
        to: to.into(),
        body: Some("Which layout should ship?".into()),
        subject: None,
        questions: vec![AskQuestion {
            id: "q1".into(),
            question_type: AskQuestionType::FreeText,
            prompt: "Which layout should ship?".into(),
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
        attachment_ids: files,
    }
}

fn answer_params(ask_id: &str, files: Vec<String>) -> ChatAnswerParams {
    ChatAnswerParams {
        ask_id: ask_id.into(),
        answers: Vec::new(),
        option_id: None,
        text: Some("The second one, annotated".into()),
        attachment_ids: files,
    }
}

/// What either party sees of the conversation's files, message by message (by ID: a rebuild
/// may renumber messages committed in the same millisecond).
type Seen = Vec<(
    String,
    String,
    Vec<AttachmentMeta>,
    Option<Vec<AttachmentMeta>>,
)>;

fn seen(views: Vec<ChatMessageView>) -> Seen {
    let mut seen: Seen = views
        .into_iter()
        .map(|m| {
            let answer = m.answer.map(|a| a.attachments);
            (m.id, m.body.unwrap_or_default(), m.attachments, answer)
        })
        .collect();
    seen.sort_by(|a, b| a.0.cmp(&b.0));
    seen
}

fn both_views(fx: &Fixture, conversation: &str) -> (Seen, Seen) {
    (
        seen(messages(&fx.office, &fx.agent, conversation)),
        seen(messages(&fx.office, &fx.person, conversation)),
    )
}

/// How many message and answer file rows the projection holds: `message_attachments` and
/// `message_answer_attachments` on SQLite, the messaging table's lists on Memory.
fn projected_files(office: &Office) -> (usize, usize) {
    let dump = office.dump();
    let count = |sqlite: &str, memory: &str| {
        dump.iter()
            .filter(|line| line.contains(sqlite) || line.contains(memory))
            .count()
    };
    (
        count("messaging.message_attachments [", "message_attachment "),
        count(
            "messaging.message_answer_attachments [",
            "answer_attachment ",
        ),
    )
}

#[test]
fn an_answers_files_persist_on_its_answer_and_its_reply_and_survive_a_rebuild() {
    for engine in ENGINES {
        let fx = Fixture::new(engine);
        let office = &fx.office;
        let mockup = png(office, &fx.agent, "mockup.png");
        let ask = office
            .msg_ask(&fx.agent, ask_params(&fx.person, vec![mockup.clone()]))
            .unwrap()
            .unwrap();
        assert_eq!(ask.message.attachments.len(), 1);
        assert!(!still_staged(office, &fx.agent, &mockup));
        let conversation = ask.result.conversation_id.clone();

        let annotated = png(office, &fx.person, "annotated.png");
        let spec = pdf(office, &fx.person, "spec.pdf");
        let answered = office
            .chat_answer(
                &fx.person,
                answer_params(&ask.result.ask_id, vec![annotated.clone(), spec.clone()]),
            )
            .unwrap()
            .unwrap();
        let on_answer = answered.result.answer.attachments.clone();
        // The ask's file and the reply's two; the answer's two.
        assert_eq!(projected_files(office), (3, 2), "{engine:?}");
        assert_eq!(on_answer.len(), 2, "{engine:?}");
        assert_eq!(on_answer[0].filename, "annotated.png");
        assert_eq!(on_answer[1].page_count, Some(3));
        assert!(!still_staged(office, &fx.person, &annotated));

        let views = messages(office, &fx.agent, &conversation);
        let ask_view = views.iter().find(|m| m.id == ask.result.ask_id).unwrap();
        assert_eq!(ask_view.attachments.len(), 1);
        let answer = ask_view.answer.as_ref().expect("answered");
        assert_eq!(answer.attachments, on_answer, "{engine:?}");
        let reply = views.iter().find(|m| m.kind == KIND_REPLY).unwrap();
        assert_eq!(reply.attachments.len(), 2);
        // Two bindings of the same bytes: two attachment IDs, one per record.
        for (on_reply, on_answer) in reply.attachments.iter().zip(&on_answer) {
            assert_ne!(on_reply.id, on_answer.id);
            assert_eq!(on_reply.filename, on_answer.filename);
            assert_eq!(on_reply.byte_count, on_answer.byte_count);
        }
        let answer_file = inspect(office, &fx.agent, &on_answer[1].id).unwrap();
        assert_eq!(answer_file.target_kind, AttachmentTargetKind::AskAnswer);
        assert_eq!(
            answer_file.target_id.as_deref(),
            Some(ask.result.ask_id.as_str())
        );
        assert_eq!(answer_file.uploaded_by_employee_id, fx.person);
        let reply_file = inspect(office, &fx.person, &reply.attachments[1].id).unwrap();
        assert_eq!(reply_file.target_kind, AttachmentTargetKind::Message);
        assert_eq!(reply_file.content_sha256, answer_file.content_sha256);
        assert_eq!(
            read_all(office, &fx.agent, &on_answer[1].id, 4096).unwrap(),
            read_all(office, &fx.person, &reply.attachments[1].id, 4096).unwrap()
        );
        for outsider in [&fx.other_person, &fx.outsider_agent] {
            for id in [
                &on_answer[0].id,
                &reply.attachments[0].id,
                &ask_view.attachments[0].id,
            ] {
                assert_eq!(
                    inspect(office, outsider, id).unwrap_err().code,
                    ErrorCode::NotFound
                );
            }
        }
        // A plain DM, a channel post and a reply, all with files, ride the rebuild too.
        let dm = send(
            office,
            &fx.person,
            &fx.agent,
            "",
            &[&pdf(office, &fx.person, "x.pdf")],
        )
        .unwrap();
        let channel = send(
            office,
            &fx.agent,
            "#general",
            "board",
            &[&png(office, &fx.agent, "b.png")],
        )
        .unwrap();
        let before = both_views(&fx, &conversation);
        let general = seen(messages(office, &fx.owner, &channel.result.conversation_id));
        assert_eq!(dm.result.conversation_id, conversation);

        assert_eq!(projected_files(office), (5, 2), "{engine:?}");
        let fx = fx.rebuilt();
        assert_eq!(projected_files(&fx.office), (5, 2), "{engine:?}: rebuilt");
        assert_eq!(
            both_views(&fx, &conversation),
            before,
            "{engine:?}: rebuilt views"
        );
        assert_eq!(
            seen(messages(
                &fx.office,
                &fx.owner,
                &channel.result.conversation_id
            )),
            general
        );
        let after = inspect(&fx.office, &fx.agent, &on_answer[1].id).unwrap();
        assert_eq!(after, answer_file);
        assert_eq!(
            inspect(&fx.office, &fx.other_person, &on_answer[1].id)
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
        assert_eq!(
            read_all(&fx.office, &fx.person, &reply.attachments[1].id, 4096).unwrap(),
            crate::attachments::media::tests::fixtures::pdf(3)
        );
    }
}

#[test]
fn answer_files_follow_the_same_rules_as_message_files() {
    for engine in ENGINES {
        let fx = Fixture::new(engine);
        let office = &fx.office;
        let ask = office
            .msg_ask(&fx.agent, ask_params(&fx.person, Vec::new()))
            .unwrap()
            .unwrap();
        // Another member's upload never goes into my answer.
        let theirs = pdf(office, &fx.agent, "theirs.pdf");
        let refused = office
            .chat_answer(
                &fx.person,
                answer_params(&ask.result.ask_id, vec![theirs.clone()]),
            )
            .unwrap()
            .unwrap_err();
        assert_eq!(refused.code, ErrorCode::NotFound, "{engine:?}");
        assert!(still_staged(office, &fx.agent, &theirs));
        let eleven: Vec<String> = (0..11).map(|_| next_uuid()).collect();
        let refused = office
            .chat_answer(&fx.person, answer_params(&ask.result.ask_id, eleven))
            .unwrap()
            .unwrap_err();
        assert_eq!(refused.code, ErrorCode::TooLarge);
        // The ask stays open: a refused answer changed nothing.
        let mine = png(office, &fx.person, "mine.png");
        let answered = office
            .chat_answer(&fx.person, answer_params(&ask.result.ask_id, vec![mine]))
            .unwrap()
            .unwrap();
        assert_eq!(answered.result.answer.attachments.len(), 1);
    }
}
