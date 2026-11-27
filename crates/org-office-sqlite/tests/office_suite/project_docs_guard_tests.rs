//! The documentation tree's guards (`TASK-55` pass 2): a satellite reads its replicated tree and
//! refuses every write; `docs.put_media` authorizes before it decodes a byte; sub-team agents
//! are project members; assets and the index stay inside one `org-sync` payload; a current
//! document keeps an owner; and an archived project's tree is read-only.

use base64::Engine as _;
use image::codecs::png::PngEncoder;
use image::{DynamicImage, Rgb, RgbImage};
use org_proto::docs::{
    DocMediaKindWire, DocStatusWire, DocsIndexParams, DocsInspectParams, DocsListParams,
    DocsMoveParams, DocsPutMediaParams, DocsUpdateParams, DOC_MEDIA_MAX_BASE64_CHARS,
    DOC_MEDIA_MAX_BYTES,
};
use org_proto::ErrorCode;
use org_store::{Ack, Change, Id, Refusal, Store, StoreError, Stream};

use super::{
    create, create_params, engines, err, every_call, every_write, fixture, index, index_file,
    media_params, next_uuid, offboard, ok, reopen, team, Fx,
};
use org_media::testkit::png;
use org_office::project_docs::index::INDEX_MAX_BYTES;
use org_office::projects::ports::ProjectsRead;
use org_office::projects::records::{encode_project, project_path};
use org_office::testkit_support::store_open::{EngineChoice, OfficeEngine, StoreOp};
use org_office::TestSatellite;

/// A `side` x `side` PNG of incompressible noise: its sanitized copy is about `3 * side²` bytes.
fn noise_png(side: u32) -> Vec<u8> {
    let mut state: u32 = 0x9e37_79b9;
    let image = RgbImage::from_fn(side, side, |_, _| {
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state.to_le_bytes()[0]
        };
        Rgb([next(), next(), next()])
    });
    let mut out = Vec::new();
    DynamicImage::ImageRgb8(image)
        .write_with_encoder(PngEncoder::new(&mut out))
        .unwrap();
    out
}

/// Marks a project archived, as `project.archive` will once the office takes it.
struct ArchiveProject(Id);

impl StoreOp for ArchiveProject {
    type Out = Result<(), StoreError>;

    fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out {
        store
            .submit(None, true, move |uow| {
                uow.next(Stream::Organization)?;
                let mut project = uow
                    .view()
                    .project(&self.0)?
                    .ok_or_else(|| Refusal::invalid("no such project"))?;
                project.archived = true;
                Ok(Change::new(Ack::Durable)
                    .put(project_path(&project.id), encode_project(&project))
                    .reply(b"{}".to_vec()))
            })
            .map(|_| ())
    }
}

fn update_params(fx: &Fx, doc: &str, revision: u64) -> DocsUpdateParams {
    DocsUpdateParams {
        command_id: next_uuid(),
        project: fx.project.clone(),
        document_id: doc.into(),
        expected_revision: revision,
        ..Default::default()
    }
}

#[test]
fn a_satellite_reads_its_replicated_tree_and_refuses_every_write_as_unavailable() {
    let fx = fixture(EngineChoice::Memory);
    let doc = create(&fx, &fx.owner, "Runbook", Some("ops/runbook.md")).document;
    ok(fx
        .office
        .docs_put_media(&fx.owner, media_params(&fx.project, "arch.png", &png(4, 4))));
    let tree = index(&fx, &fx.owner);
    let files = fx.office.collect_authoritative_state_snapshot().unwrap();
    assert!(files.iter().any(|(p, _)| p.ends_with("/docs/index.yaml")));
    let info = fx.office.info();
    let sat = TestSatellite::joined_to(&info.org_id, &info.office_id);
    let batch = files.into_iter().map(|(p, b)| (p, Some(b))).collect();
    sat.office
        .apply_replicated_state_files(&next_uuid(), batch)
        .unwrap();
    let office = &sat.office;
    let project = || fx.project.clone();
    let replicated = ok(office.docs_index(&fx.owner, DocsIndexParams { project: project() }));
    assert_eq!(replicated, tree);
    let listed = ok(office.docs_list(
        &fx.owning_agent,
        DocsListParams {
            project: project(),
            ..Default::default()
        },
    ));
    assert_eq!(listed.documents.len(), 1);
    let inspected = ok(office.docs_inspect(
        &fx.owner,
        DocsInspectParams {
            project: project(),
            document: doc.id.clone(),
        },
    ));
    assert_eq!(inspected.body_markdown, "# Runbook\n");
    // An outsider is still told nothing on a satellite.
    let hidden = err(office.docs_index(&fx.outsider_agent, DocsIndexParams { project: project() }));
    assert_eq!(
        (hidden.code, hidden.message.as_str()),
        (ErrorCode::NotFound, "project not found")
    );

    let refusals = [
        (
            "create",
            err(office.docs_create(&fx.owner, create_params(&fx.project, "Local", None))),
        ),
        ("update", {
            let mut params = update_params(&fx, &doc.id, 1);
            params.title = Some("Forked".into());
            err(office.docs_update(&fx.owner, params))
        }),
        (
            "move",
            err(office.docs_move(
                &fx.owner,
                DocsMoveParams {
                    command_id: next_uuid(),
                    project: project(),
                    document_id: doc.id.clone(),
                    expected_revision: 1,
                    new_path: Some("elsewhere.md".into()),
                    ..Default::default()
                },
            )),
        ),
        (
            "put_media",
            err(office.docs_put_media(&fx.owner, media_params(&fx.project, "x.png", &png(2, 2)))),
        ),
    ];
    for (call, refusal) in refusals {
        assert_eq!(
            refusal.code,
            ErrorCode::Unavailable,
            "{call} on a satellite"
        );
    }
    assert_eq!(
        ok(office.docs_index(&fx.owner, DocsIndexParams { project: project() })),
        tree,
        "a refused write changes nothing"
    );
    let sat = sat.rebuilt();
    assert_eq!(
        ok(sat
            .office
            .docs_index(&fx.owner, DocsIndexParams { project: project() })),
        tree,
        "the replicated tree rebuilds on the satellite"
    );
    sat.office.close().unwrap();
    fx.office.close().unwrap();
}

/// Media params that would each fail validation, were the caller ever let that far.
fn hostile_media(project: &str) -> Vec<(&'static str, DocsPutMediaParams)> {
    let base = || media_params(project, "x.png", &png(2, 2));
    let mut malformed = base();
    malformed.content_base64 = "***".into();
    let mut oversized = base();
    oversized.content_base64 = "A".repeat(DOC_MEDIA_MAX_BASE64_CHARS + 4);
    let mut svg = base();
    svg.file_name = "evil.svg".into();
    svg.media_type = "image/svg+xml".into();
    svg.content_base64 = base64::engine::general_purpose::STANDARD.encode(b"<svg onload=alert(1)>");
    let mut traversal = base();
    traversal.file_name = "../../escape.png".into();
    let mut unknown = base();
    unknown.kind = DocMediaKindWire::Unknown;
    vec![
        ("malformed", malformed),
        ("oversized", oversized),
        ("svg", svg),
        ("traversal", traversal),
        ("unknown kind", unknown),
    ]
}

#[test]
fn put_media_refuses_outsiders_and_inactive_callers_before_decoding_a_byte() {
    for engine in engines() {
        let fx = fixture(engine.clone());
        let stranger = next_uuid();
        offboard(&fx, &fx.contributing_agent);
        for (caller, expected) in [
            (&fx.outsider_agent, ErrorCode::NotFound),
            (&fx.contributing_agent, ErrorCode::NotAuthorized),
            (&stranger, ErrorCode::NotAuthorized),
        ] {
            let uniform = err(fx
                .office
                .docs_put_media(caller, media_params(&fx.project, "ok.png", &png(2, 2))));
            assert_eq!(uniform.code, expected, "{engine:?}");
            if expected == ErrorCode::NotFound {
                assert_eq!(uniform.message, "project not found");
            }
            for (what, params) in hostile_media(&fx.project) {
                let refusal = err(fx.office.docs_put_media(caller, params));
                assert_eq!(refusal, uniform, "{engine:?} {what} as {caller}");
            }
        }
        // A member's same requests reach validation.
        for (what, params) in hostile_media(&fx.project) {
            let refusal = err(fx.office.docs_put_media(&fx.owning_agent, params));
            assert!(
                !matches!(refusal.code, ErrorCode::NotFound | ErrorCode::NotAuthorized),
                "{engine:?} {what}: {refusal:?}"
            );
        }
        assert!(index(&fx, &fx.owner).media.is_empty());
        fx.office.close().unwrap();
    }
}

#[test]
fn agents_of_a_sub_team_of_an_owning_or_contributing_team_are_project_members() {
    for engine in engines() {
        let fx = fixture(engine.clone());
        let teams = fx.office.org_snapshot().unwrap().teams;
        let named = |name: &str| teams.iter().find(|t| t.name == name).unwrap().id.clone();
        let (core, design) = (named("Platform Core"), named("Design Ops"));
        let infra = team(&fx.office, &fx.owner, "Platform Core Infra", &core);
        let ux = team(&fx.office, &fx.owner, "Design Ops UX", &design);
        let deep = super::agent(&fx.office, &fx.owner, &infra, "Infra Bot", "infra-bot");
        let ux_agent = super::agent(&fx.office, &fx.owner, &ux, "UX Bot", "ux-bot");
        let doc = create(&fx, &fx.owner, "Plan", Some("plan.md")).document;
        for member in [&fx.child_agent, &deep, &ux_agent] {
            assert!(index(&fx, member).documents.iter().any(|d| d.id == doc.id));
            let mut params = create_params(&fx.project, "By a sub-team", None);
            params.section = Some(format!("by-{}", &member[member.len() - 4..]));
            ok(fx.office.docs_create(member, params));
            let file = format!("{}.png", &member[member.len() - 4..]);
            ok(fx
                .office
                .docs_put_media(member, media_params(&fx.project, &file, &png(2, 2))));
            // Membership flows down a team chain, never sideways to another project.
            let other = err(fx.office.docs_index(
                member,
                DocsIndexParams {
                    project: fx.other_project.clone(),
                },
            ));
            assert_eq!(
                (other.code, other.message.as_str()),
                (ErrorCode::NotFound, "project not found")
            );
        }
        for (call, refusal) in every_call(&fx, &fx.outsider_agent, &fx.project, &doc.id) {
            assert_eq!(
                (refusal.code, refusal.message.as_str()),
                (ErrorCode::NotFound, "project not found"),
                "{engine:?} {call}"
            );
        }
        fx.office.close().unwrap();
    }
}

#[test]
fn media_and_the_index_stay_inside_one_org_sync_payload_or_are_too_large() {
    let fx = fixture(EngineChoice::Memory);
    let put = |params| fx.office.docs_put_media(&fx.owner, params);
    // Just over the cap, decoded: refused before the decoder runs.
    let over = vec![0u8; DOC_MEDIA_MAX_BYTES + 1];
    assert_eq!(
        err(put(media_params(&fx.project, "over.png", &over))).code,
        ErrorCode::TooLarge
    );
    let big = noise_png(360);
    assert!(big.len() > DOC_MEDIA_MAX_BYTES);
    assert_eq!(
        err(put(media_params(&fx.project, "big.png", &big))).code,
        ErrorCode::TooLarge
    );
    // An asset near the cap is kept, and its entry replicates.
    let near = noise_png(340);
    assert!(near.len() <= DOC_MEDIA_MAX_BYTES);
    let kept = ok(put(media_params(&fx.project, "near.png", &near))).media;
    assert!(kept.bytes <= DOC_MEDIA_MAX_BYTES as u64);
    assert!(fx.office.sync_org_journal_to_replication().unwrap() > 0);

    // Documents with long titles and summaries until the index would pass 256 KiB.
    let mut created = 0usize;
    let refusal = loop {
        assert!(created < 400, "the index cap was never reached");
        let mut params = create_params(
            &fx.project,
            &format!("{created:03} {}", "T".repeat(180)),
            None,
        );
        params.section = Some(format!("s{:03}", created % 7));
        params.summary = Some("s".repeat(2000));
        match fx.office.docs_create(&fx.owner, params).unwrap() {
            Ok(_) => created += 1,
            Err(refusal) => break refusal,
        }
    };
    assert_eq!(refusal.code, ErrorCode::TooLarge);
    assert!(created > 50, "{created}");
    fx.office.sync_store().unwrap();
    let on_disk = std::fs::metadata(index_file(&fx)).unwrap().len();
    assert!(on_disk <= INDEX_MAX_BYTES as u64, "{on_disk}");
    assert_eq!(index(&fx, &fx.owner).documents.len(), created);
    assert!(fx.office.sync_org_journal_to_replication().unwrap() > 0);
    let fx = reopen(fx).expect("the capped tree rebuilds");
    assert_eq!(index(&fx, &fx.owner).documents.len(), created);
    fx.office.close().unwrap();
}

#[test]
fn a_current_documents_owner_is_never_cleared() {
    for engine in engines() {
        let fx = fixture(engine.clone());
        let mut params = create_params(&fx.project, "Runbook", Some("runbook.md"));
        params.status = Some(DocStatusWire::Current);
        let doc = ok(fx.office.docs_create(&fx.owning_agent, params)).document;
        assert_eq!(
            doc.owner_employee_id.as_deref(),
            Some(fx.owning_agent.as_str())
        );
        for cleared in ["", "   "] {
            let mut params = update_params(&fx, &doc.id, 1);
            params.owner_employee_id = Some(cleared.into());
            let refusal = err(fx.office.docs_update(&fx.owner, params));
            assert_eq!(refusal.code, ErrorCode::InvalidRequest, "{engine:?}");
            assert!(refusal.message.starts_with("owner_employee_id:"));
        }
        let unchanged = &index(&fx, &fx.owner).documents[0];
        assert_eq!(unchanged.revision, 1);
        assert_eq!(
            unchanged.owner_employee_id.as_deref(),
            Some(fx.owning_agent.as_str())
        );
        // Made a draft in the same update, its owner may go.
        let mut params = update_params(&fx, &doc.id, 1);
        params.status = Some(DocStatusWire::Draft);
        params.owner_employee_id = Some(String::new());
        let drafted = ok(fx.office.docs_update(&fx.owner, params)).document;
        assert_eq!(drafted.owner_employee_id, None);
        fx.office.close().unwrap();
    }
}

#[test]
fn an_archived_projects_tree_is_read_only_and_still_hidden_from_outsiders() {
    for engine in engines() {
        let fx = fixture(engine.clone());
        let doc = create(&fx, &fx.owner, "Handbook", Some("handbook.md")).document;
        ok(fx
            .office
            .docs_put_media(&fx.owner, media_params(&fx.project, "arch.png", &png(4, 4))));
        let tree = index(&fx, &fx.owner);
        fx.office
            .store_for_test()
            .run(ArchiveProject(Id::parse(&fx.project).unwrap()))
            .unwrap();
        for reader in [&fx.owner, &fx.person, &fx.owning_agent, &fx.child_agent] {
            assert_eq!(index(&fx, reader), tree, "{engine:?}");
            ok(fx.office.docs_list(
                reader,
                DocsListParams {
                    project: fx.project.clone(),
                    ..Default::default()
                },
            ));
            ok(fx.office.docs_inspect(
                reader,
                DocsInspectParams {
                    project: fx.project.clone(),
                    document: doc.id.clone(),
                },
            ));
            for (call, refusal) in every_write(&fx, reader, &fx.project, &doc.id) {
                assert_eq!(refusal.code, ErrorCode::NotAuthorized, "{engine:?} {call}");
            }
        }
        for (call, refusal) in every_call(&fx, &fx.outsider_agent, &fx.project, &doc.id) {
            assert_eq!(
                (refusal.code, refusal.message.as_str()),
                (ErrorCode::NotFound, "project not found"),
                "{engine:?} {call} as an outsider"
            );
        }
        // Hostile media on an archived project is refused before it is decoded.
        for (what, params) in hostile_media(&fx.project) {
            let refusal = err(fx.office.docs_put_media(&fx.owner, params));
            assert_eq!(refusal.code, ErrorCode::NotAuthorized, "{engine:?} {what}");
        }
        let fx = reopen(fx).expect("the archived tree rebuilds");
        assert_eq!(index(&fx, &fx.owner), tree, "{engine:?}");
        fx.office.close().unwrap();
    }
}
