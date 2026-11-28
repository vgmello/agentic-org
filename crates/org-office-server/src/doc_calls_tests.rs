//! `docs.*` dispatch against a real office on both engines: the bound member runs every call,
//! an unbound connection and bad params are refused before the office, and an agent outside
//! the project is told `not_found` on every call.

use base64::Engine as _;
use org_office::{EngineChoice, Office, TestOffice};
use org_proto::docs::{
    DocMediaKindWire, DocsCreate, DocsCreateParams, DocsIndex, DocsIndexParams, DocsInspect,
    DocsInspectParams, DocsList, DocsListParams, DocsMove, DocsMoveParams, DocsPutMedia,
    DocsPutMediaParams, DocsUpdate, DocsUpdateParams,
};
use org_proto::hiring::{HireProposalWire, HrHire, HrHireParams};
use org_proto::org::{ProjectCreateParams, TeamCreateParams};
use org_proto::{Call, ErrorCode, Outcome, Request, Response};
use serde_json::{json, Value};

use super::dispatch_doc_rpc;

fn uuid(n: u64) -> String {
    format!("018bce83-1200-7000-8000-{n:012x}")
}

fn team(office: &Office, owner: &str, id: &str, name: &str, parent: &str) {
    office
        .submit_org(
            owner,
            org_office::rules::OrgCommand::TeamCreate(TeamCreateParams {
                command_id: uuid(9_000 + u64::from(name.len() as u32)),
                team_id: id.into(),
                name: name.into(),
                owner_team_id: Some(parent.into()),
            }),
        )
        .unwrap()
        .unwrap();
}

fn hire(office: &Office, owner: &str, id: &str, team_id: &str, handle: &str, n: u64) {
    let request = Request::new::<HrHire>(
        1,
        &HrHireParams {
            command_id: uuid(n),
            request_id: uuid(n + 1),
            employee_id: id.into(),
            home_team_id: team_id.into(),
            role_id: None,
            proposal: HireProposalWire {
                name: handle.into(),
                handle: handle.into(),
                ..Default::default()
            },
            reason: "documentation dispatch test".into(),
        },
    )
    .unwrap();
    let answer = office.dispatch_hiring_rpc(Some(owner), &request).unwrap();
    assert!(matches!(answer.outcome, Outcome::Result(_)), "{answer:?}");
}

struct Seeded {
    office: Office,
    owner: String,
    project: String,
    member_agent: String,
    outsider_agent: String,
}

fn seed(engine: EngineChoice) -> Seeded {
    let office = Office::for_test(TestOffice { engine });
    let owner = office.owner_member_id().to_owned();
    let root = office.org_snapshot().unwrap().teams[0].id.clone();
    let (platform, finance) = (uuid(1), uuid(2));
    team(&office, &owner, &platform, "Platform", &root);
    team(&office, &owner, &finance, "Finance Team", &root);
    let project = uuid(3);
    office
        .submit_org(
            &owner,
            org_office::rules::OrgCommand::ProjectCreate(ProjectCreateParams {
                command_id: uuid(4),
                project_id: project.clone(),
                name: "Checkout".into(),
                key: None,
                description: None,
                teams: vec![],
                general_area_id: String::new(),
                general_area_name: None,
                repo: None,
                integration_branch: None,
                slug: Some("checkout".into()),
                owner_team_id: Some(platform.clone()),
                create_team_id: None,
                parent_team_id: None,
                contributing_team_ids: vec![],
            }),
        )
        .unwrap()
        .unwrap();
    let (member_agent, outsider_agent) = (uuid(5), uuid(6));
    hire(
        &office,
        &owner,
        &member_agent,
        &platform,
        "platform-bot",
        100,
    );
    hire(
        &office,
        &owner,
        &outsider_agent,
        &finance,
        "finance-bot",
        200,
    );
    Seeded {
        office,
        owner,
        project,
        member_agent,
        outsider_agent,
    }
}

fn call<C: Call>(office: &Office, member: Option<&str>, params: &C::Params) -> Response {
    let request = Request::new::<C>(7, params).unwrap();
    dispatch_doc_rpc(office, member, &request).expect("a docs.* method is dispatched")
}

fn result(response: Response) -> Value {
    match response.outcome {
        Outcome::Result(value) => value,
        Outcome::Error(error) => panic!("refused: {error:?}"),
    }
}

fn code(response: &Response) -> ErrorCode {
    match &response.outcome {
        Outcome::Error(error) => error.code,
        Outcome::Result(value) => panic!("answered: {value}"),
    }
}

/// A 1x1 RGBA PNG.
fn png() -> String {
    let encoded = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .unwrap();
    assert!(bytes.starts_with(b"\x89PNG"));
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

#[test]
fn every_docs_call_dispatches_as_the_bound_member_on_both_engines() {
    for engine in [
        EngineChoice::Memory,
        EngineChoice::Sqlite(org_office_sqlite::sqlite_backend()),
    ] {
        let s = seed(engine);
        let me = Some(s.member_agent.as_str());
        let created = result(call::<DocsCreate>(
            &s.office,
            me,
            &DocsCreateParams {
                command_id: uuid(300),
                project: "checkout".into(),
                kind: "guide".into(),
                title: "Setup".into(),
                section: Some("guides".into()),
                body_markdown: "# Setup\n".into(),
                ..Default::default()
            },
        ));
        let doc = created["document"]["id"].as_str().unwrap().to_owned();
        assert_eq!(created["document"]["path"], "guides/setup.md");
        let inspected = result(call::<DocsInspect>(
            &s.office,
            me,
            &DocsInspectParams {
                project: s.project.clone(),
                document: doc.clone(),
            },
        ));
        assert_eq!(inspected["body_markdown"], "# Setup\n");
        let listed = result(call::<DocsList>(
            &s.office,
            Some(&s.owner),
            &DocsListParams {
                project: "P-1".into(),
                ..Default::default()
            },
        ));
        assert_eq!(listed["documents"].as_array().unwrap().len(), 1);
        let updated = result(call::<DocsUpdate>(
            &s.office,
            me,
            &DocsUpdateParams {
                command_id: uuid(301),
                project: s.project.clone(),
                document_id: doc.clone(),
                expected_revision: 1,
                status: Some(org_proto::docs::DocStatusWire::Current),
                ..Default::default()
            },
        ));
        assert_eq!(
            updated["document"]["owner_employee_id"],
            json!(s.member_agent)
        );
        let moved = result(call::<DocsMove>(
            &s.office,
            me,
            &DocsMoveParams {
                command_id: uuid(302),
                project: s.project.clone(),
                document_id: doc.clone(),
                expected_revision: 2,
                new_path: Some("setup.md".into()),
                ..Default::default()
            },
        ));
        assert_eq!(moved["document"]["section"], "");
        let media = result(call::<DocsPutMedia>(
            &s.office,
            me,
            &DocsPutMediaParams {
                command_id: uuid(303),
                project: s.project.clone(),
                file_name: "flow.png".into(),
                media_type: "image/png".into(),
                kind: DocMediaKindWire::Image,
                title: "Flow".into(),
                summary: None,
                content_base64: png(),
                expected_revision: None,
            },
        ));
        assert_eq!(media["media"]["type"], "image");
        let tree = result(call::<DocsIndex>(
            &s.office,
            me,
            &DocsIndexParams {
                project: s.project.clone(),
            },
        ));
        assert_eq!(tree["documents"][0]["path"], "setup.md");
        assert_eq!(tree["media"][0]["path"], "media/flow.png");
        assert!(tree["index_path"]
            .as_str()
            .unwrap()
            .ends_with("/docs/index.yaml"));

        // Outside the project: the same `not_found` as a project that does not exist.
        let outsider = Some(s.outsider_agent.as_str());
        for project in [s.project.as_str(), "checkout", "P-1", uuid(999).as_str()] {
            let refused = call::<DocsIndex>(
                &s.office,
                outsider,
                &DocsIndexParams {
                    project: project.into(),
                },
            );
            assert_eq!(code(&refused), ErrorCode::NotFound, "{project}");
            let Outcome::Error(error) = refused.outcome else {
                unreachable!()
            };
            assert_eq!(error.message, "project not found");
        }
        let refused = call::<DocsInspect>(
            &s.office,
            outsider,
            &DocsInspectParams {
                project: s.project.clone(),
                document: doc,
            },
        );
        assert_eq!(code(&refused), ErrorCode::NotFound);
    }
}

#[test]
fn unbound_connections_bad_params_and_other_methods_are_refused_before_the_office() {
    let s = seed(EngineChoice::Memory);
    let unbound = call::<DocsIndex>(
        &s.office,
        None,
        &DocsIndexParams {
            project: s.project.clone(),
        },
    );
    assert_eq!(code(&unbound), ErrorCode::NotBound);
    let bad = Request {
        id: 9,
        method: DocsCreate::METHOD.into(),
        params: json!({"title": 42}),
        principal: None,
    };
    let refused = dispatch_doc_rpc(&s.office, Some(&s.owner), &bad).unwrap();
    assert_eq!(code(&refused), ErrorCode::InvalidRequest);
    let other = Request::new::<org_proto::processes::SkillList>(
        3,
        &org_proto::processes::SkillListParams {},
    )
    .unwrap();
    assert!(dispatch_doc_rpc(&s.office, Some(&s.owner), &other).is_none());
}
