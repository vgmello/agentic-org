//! The attachment facade on a real office, on both engines (`TASK-98.1` AC #1-#5): uploads,
//! bindings, the policy, the private space, and lossless rebuilds.

mod acl;
mod answers;
mod custody;
mod flows;
mod ingest;
mod messages;
mod rebuild;
mod update_meta;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use base64::{engine::general_purpose::STANDARD, Engine as _};
use org_proto::attachments::{
    AttachmentAttachParams, AttachmentReadChunkParams, AttachmentTargetKind,
    AttachmentUploadBeginParams, AttachmentUploadChunkParams, AttachmentUploadRef, AttachmentView,
    StagedUploadView, ATTACHMENT_CHUNK_MAX_BYTES,
};
use org_proto::org::{MemberAddPersonParams, ProjectCreateParams, TeamCreateParams};
use org_proto::ApiError;
use org_store::testkit::{identity, FakeCustody, FixedClock};

use crate::office::testkit::{places, record, request, CA_SEED};
use crate::office::{Custody, DataRoots, Office, Options};
use crate::organization::testkit::CREATION_AT;
use crate::rules::OrgCommand;
use crate::store::open::EngineChoice;

pub(super) const ENGINES: [EngineChoice; 2] = [EngineChoice::Memory, EngineChoice::Sqlite];

static NEXT_ID: AtomicU64 = AtomicU64::new(120_000);

pub(super) fn next_uuid() -> String {
    let n = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let ms = CREATION_AT.0 + n;
    let mut b = [0u8; 16];
    b[0..6].copy_from_slice(&ms.to_be_bytes()[2..8]);
    b[6] = 0x70 | ((n >> 8) as u8 & 0x0f);
    b[7] = (n & 0xff) as u8;
    b[8] = 0x80;
    b[14] = ((n >> 8) & 0xff) as u8;
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

pub(super) struct Fixture {
    pub tmp: tempfile::TempDir,
    pub folder: std::path::PathBuf,
    pub roots: DataRoots,
    pub custody: Custody,
    pub engine: EngineChoice,
    pub office: Office,
    /// The organization's owner (a person).
    pub owner: String,
    /// A person on the project's team.
    pub person: String,
    /// A person on another team.
    pub other_person: String,
    /// An agent on the project's team.
    pub agent: String,
    /// An agent on another team.
    pub outsider_agent: String,
    pub project: String,
}

fn options(engine: EngineChoice) -> Options {
    Options {
        engine,
        clock: Arc::new(FixedClock(CREATION_AT)),
        testing: Default::default(),
        before_final_checks: None,
    }
}

fn team(office: &Office, owner: &str, parent: &str, name: &str) -> String {
    let team_id = next_uuid();
    office
        .submit_org(
            owner,
            OrgCommand::TeamCreate(TeamCreateParams {
                command_id: next_uuid(),
                team_id: team_id.clone(),
                name: name.into(),
                owner_team_id: Some(parent.into()),
            }),
        )
        .unwrap()
        .unwrap();
    team_id
}

fn add_person(office: &Office, owner: &str, team: &str, name: &str, handle: &str) -> String {
    let member_id = next_uuid();
    office
        .submit_org(
            owner,
            OrgCommand::MemberAddPerson(MemberAddPersonParams {
                command_id: next_uuid(),
                member_id: member_id.clone(),
                name: name.into(),
                handle: Some(handle.into()),
                home_team_id: Some(team.into()),
                contact: None,
            }),
        )
        .unwrap()
        .unwrap();
    member_id
}

fn hire_agent(office: &Office, owner: &str, team: &str, name: &str, handle: &str) -> String {
    use org_proto::hiring::{HireProposalWire, HrHire, HrHireParams};
    use org_proto::{Outcome, Request};
    let employee_id = next_uuid();
    let req = Request::new::<HrHire>(
        1,
        &HrHireParams {
            command_id: next_uuid(),
            request_id: next_uuid(),
            employee_id: employee_id.clone(),
            home_team_id: team.into(),
            role_id: None,
            proposal: HireProposalWire {
                name: name.into(),
                handle: handle.into(),
                title_in_team: Some("Engineer".into()),
                ..Default::default()
            },
            reason: "attachment tests".into(),
        },
    )
    .unwrap();
    let resp = office.dispatch_hiring_rpc(Some(owner), &req).unwrap();
    assert!(matches!(resp.outcome, Outcome::Result(_)), "{resp:?}");
    employee_id
}

impl Fixture {
    pub fn new(engine: EngineChoice) -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let (folder, roots) = places(tmp.path());
        let custody = Custody {
            identity: identity(),
            authority: Arc::new(FakeCustody::new()),
        };
        let (new, binding) = request();
        let office = Office::provision_with(
            folder.clone(),
            &new,
            None,
            &binding,
            roots.clone(),
            custody.clone(),
            |_| record(CA_SEED),
            options(engine),
        )
        .unwrap();
        let owner = office.owner_member_id().to_owned();
        let root = office.org_snapshot().unwrap().teams[0].id.clone();
        let project_team = team(&office, &owner, &root, "Platform");
        let other_team = team(&office, &owner, &root, "Design");
        let project = next_uuid();
        office
            .submit_org(
                &owner,
                OrgCommand::ProjectCreate(ProjectCreateParams {
                    command_id: next_uuid(),
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
                    owner_team_id: Some(project_team.clone()),
                    create_team_id: None,
                    parent_team_id: None,
                    contributing_team_ids: vec![],
                }),
            )
            .unwrap()
            .unwrap();
        let person = add_person(&office, &owner, &project_team, "Pat Platform", "pat");
        let other_person = add_person(&office, &owner, &other_team, "Dana Design", "dana");
        let agent = hire_agent(&office, &owner, &project_team, "Builder Bot", "builder-bot");
        let outsider_agent = hire_agent(&office, &owner, &other_team, "Sketch Bot", "sketch-bot");
        Self {
            tmp,
            folder,
            roots,
            custody,
            engine,
            office,
            owner,
            person,
            other_person,
            agent,
            outsider_agent,
            project,
        }
    }

    /// Closes the office, deletes its runtime projection and opens it again: every
    /// projection is rebuilt from the durable files and the journal.
    pub fn rebuilt(self) -> Self {
        let Self {
            tmp,
            folder,
            roots,
            custody,
            engine,
            office,
            ..
        } = self;
        let rest = (
            self.owner,
            self.person,
            self.other_person,
            self.agent,
            self.outsider_agent,
            self.project,
        );
        office.close().unwrap();
        let projection = roots
            .runtime
            .join(identity().office_id.as_str())
            .join("office.db");
        if engine == EngineChoice::Sqlite {
            assert!(projection.exists());
        }
        std::fs::remove_dir_all(&roots.runtime).unwrap();
        std::fs::create_dir_all(&roots.runtime).unwrap();
        let office = Office::open_with(
            folder.clone(),
            None,
            &identity().instance_id.to_string(),
            roots.clone(),
            custody.clone(),
            options(engine),
        )
        .unwrap();
        Self {
            tmp,
            folder,
            roots,
            custody,
            engine,
            office,
            owner: rest.0,
            person: rest.1,
            other_person: rest.2,
            agent: rest.3,
            outsider_agent: rest.4,
            project: rest.5,
        }
    }
}

/// Uploads `bytes` in chunks of at most 512 KiB and commits them.
pub(super) fn upload(
    office: &Office,
    actor: &str,
    filename: &str,
    mime: &str,
    bytes: &[u8],
) -> Result<StagedUploadView, ApiError> {
    let begun = office
        .attachment_upload_begin(
            actor,
            AttachmentUploadBeginParams {
                filename: filename.into(),
                mime_type: mime.into(),
                byte_count: bytes.len() as u64,
            },
        )
        .unwrap()?;
    assert_eq!(begun.chunk_size_bytes, ATTACHMENT_CHUNK_MAX_BYTES);
    let id = begun.upload.upload_id;
    let mut offset = 0usize;
    for chunk in bytes.chunks(begun.chunk_size_bytes as usize) {
        let sent = office
            .attachment_upload_chunk(
                actor,
                AttachmentUploadChunkParams {
                    upload_id: id.clone(),
                    offset: offset as u64,
                    data_base64: STANDARD.encode(chunk),
                },
            )
            .unwrap()?;
        offset += chunk.len();
        assert_eq!(sent.received_bytes, offset as u64);
        assert_eq!(sent.complete, offset == bytes.len());
    }
    let committed = office
        .attachment_upload_commit(actor, AttachmentUploadRef { upload_id: id })
        .unwrap()?;
    assert!(committed.upload.committed);
    Ok(committed.upload)
}

pub(super) fn attach_params(
    kind: AttachmentTargetKind,
    target: Option<&str>,
) -> AttachmentAttachParams {
    AttachmentAttachParams {
        command_id: next_uuid(),
        upload_id: None,
        source_attachment_id: None,
        target_kind: kind,
        target_id: target.map(str::to_owned),
        title: None,
        description: None,
        artifact_status: None,
    }
}

/// Binds a committed upload to a target.
pub(super) fn attach_upload(
    office: &Office,
    actor: &str,
    upload_id: &str,
    kind: AttachmentTargetKind,
    target: Option<&str>,
) -> Result<AttachmentView, ApiError> {
    let mut params = attach_params(kind, target);
    params.upload_id = Some(upload_id.to_owned());
    office
        .attachment_attach(actor, params)
        .unwrap()
        .map(|r| r.attachment)
}

/// Re-attaches an existing attachment to a target.
pub(super) fn reattach(
    office: &Office,
    actor: &str,
    source: &str,
    kind: AttachmentTargetKind,
    target: Option<&str>,
) -> Result<AttachmentView, ApiError> {
    let mut params = attach_params(kind, target);
    params.source_attachment_id = Some(source.to_owned());
    office
        .attachment_attach(actor, params)
        .unwrap()
        .map(|r| r.attachment)
}

/// Reads a whole attachment back in chunks of `chunk` bytes.
pub(super) fn read_all(
    office: &Office,
    actor: &str,
    attachment_id: &str,
    chunk: u32,
) -> Result<Vec<u8>, ApiError> {
    let mut out = Vec::new();
    loop {
        let read = office
            .attachment_read_chunk(
                actor,
                AttachmentReadChunkParams {
                    attachment_id: attachment_id.into(),
                    offset: out.len() as u64,
                    max_bytes: Some(chunk),
                },
            )
            .unwrap()?;
        assert!(
            read.data_base64.len() <= org_proto::attachments::ATTACHMENT_CHUNK_MAX_BASE64_CHARS
        );
        out.extend(STANDARD.decode(read.data_base64).unwrap());
        if read.eof {
            assert_eq!(out.len() as u64, read.total_bytes);
            return Ok(out);
        }
    }
}

/// A CSV larger than two chunks.
pub(super) fn big_csv() -> Vec<u8> {
    let mut out = b"id,name,amount\n".to_vec();
    let mut n = 0u64;
    while out.len() < 1_300_000 {
        out.extend(format!("{n},row {n},{}\n", n * 7 % 1000).into_bytes());
        n += 1;
    }
    out
}
