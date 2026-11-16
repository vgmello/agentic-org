//! Selected application acceptance. Host authority is a trusted port, separate
//! from command/adoption data. No transport capability is published here.

use super::{AcceptanceIdentity, TaskSnapshot, Updater};
use crate::{
    store::{origin, Store},
    OfficeError,
};
use org_work::selected_assignment::{
    self as assignment, AssignmentCommand, AssignmentScope, DesiredTargetFacts, EmployeeFact,
    TeamFact,
};
use org_work::{
    selected_codec::{self as codec, Json},
    selected_history::{self as history, Admission, EventFields},
};
use rusqlite::{params, Connection, OptionalExtension};
use std::{collections::BTreeMap, sync::Arc};

/// Returned only by a protected host session/grant/account verifier. It must
/// recheck channel, session expiry/revocation, linked scope and live ownership.
/// Implementations must not derive this identity from imported profile flags.
pub trait OwnerAuthority: Send + Sync + 'static {
    fn verify_live(&self, scope: &AcceptanceIdentity) -> Result<OwnerAccount, OfficeError>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OwnerAccount {
    pub account_id: String,
    pub member_id: String,
}

pub struct AssignmentRequest {
    pub operation_id: String,
    pub task_id: String,
    pub command: AssignmentCommand,
}

pub type AssignmentOutcome = org_proto::selected_tasks::AssignmentResult<i64>;

fn storage(e: impl std::error::Error + Send + Sync + 'static) -> OfficeError {
    OfficeError::storage(e)
}
fn invalid() -> OfficeError {
    OfficeError::Invalid("selected state unavailable")
}
fn unauthorized() -> OfficeError {
    OfficeError::Invalid("not authorized")
}

pub(super) fn owner(
    db: &Connection,
    id: &AcceptanceIdentity,
    authority: &dyn OwnerAuthority,
) -> Result<OwnerAccount, OfficeError> {
    let owner = authority.verify_live(id)?;
    if owner.account_id.is_empty() || owner.member_id.is_empty() {
        return Err(unauthorized());
    }
    if !owner_binding(db, id, &owner)? {
        return Err(unauthorized());
    }
    Ok(owner)
}
fn owner_binding(
    db: &Connection,
    id: &AcceptanceIdentity,
    subject: &OwnerAccount,
) -> Result<bool, OfficeError> {
    db.query_row("SELECT EXISTS(SELECT 1 FROM selected_owner_authority WHERE organization_id=?1 AND account_id=?2 AND member_id=?3 AND active=1)",params![id.organization_id,subject.account_id,subject.member_id],|r|r.get(0)).map_err(storage)
}

pub(super) fn now(db: &Connection) -> Result<u64, OfficeError> {
    let retained: Option<i64> = db
        .query_row("SELECT high_water_ms FROM work_clock WHERE id=1", [], |r| {
            r.get(0)
        })
        .optional()
        .map_err(storage)?;
    let retained = u64::try_from(retained.unwrap_or(0)).map_err(|_| invalid())?;
    let now = crate::time::now_ms().max(retained);
    if now > i64::MAX as u64 {
        return Err(invalid());
    }
    Ok(now)
}

struct Receipt {
    instance: String,
    account: String,
    method: String,
    version: i64,
    fingerprint: String,
    result: Vec<u8>,
    expires: i64,
}
fn receipt(
    db: &Connection,
    id: &AcceptanceIdentity,
    operation: &str,
    owner: &OwnerAccount,
    fingerprint: Option<&str>,
) -> Result<Option<AssignmentOutcome>, OfficeError> {
    let stored=db.query_row("SELECT instance_id,account_id,method,command_version,fingerprint,result,expires_at_ms FROM selected_receipts WHERE organization_id=?1 AND operation_id=?2",params![id.organization_id,operation],|r|Ok(Receipt {instance:r.get(0)?,account:r.get(1)?,method:r.get(2)?,version:r.get(3)?,fingerprint:r.get(4)?,result:r.get(5)?,expires:r.get(6)?})).optional().map_err(storage)?;
    let Some(r) = stored else {
        if fingerprint.is_some() {
            let legacy: bool = db
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM work_change_results WHERE change_id=?1)",
                    [operation],
                    |r| r.get(0),
                )
                .map_err(storage)?;
            if legacy {
                return Err(OfficeError::Invalid("operation reused"));
            }
        }
        return Ok(None);
    };
    // Bind before decoding or disclosing any result, including expiry.
    if r.instance != id.instance_id
        || r.account != owner.account_id
        || r.method != "task.set_assignment"
        || r.version != 1
        || fingerprint.is_some_and(|f| f != r.fingerprint)
    {
        return Err(OfficeError::Invalid("operation reused"));
    }
    if now(db)? >= u64::try_from(r.expires).map_err(|_| invalid())? {
        return Err(OfficeError::Invalid("operation expired"));
    }
    Ok(Some(
        serde_json::from_slice(&r.result).map_err(|_| invalid())?,
    ))
}

pub(crate) fn lookup(
    store: &Store,
    authority: Arc<dyn OwnerAuthority>,
    operation: String,
) -> Result<Option<AssignmentOutcome>, OfficeError> {
    if crate::ulid::time_ms(&operation).is_none() {
        return Err(OfficeError::Invalid("invalid operation identity"));
    }
    store.selected_read(move |db, id| {
        // Authentication failure remains a denial. A live scoped subject has
        // no visibility of another account's protected operation identity:
        // return the exact missing-record result before reading any receipt.
        let subject = authority.verify_live(id)?;
        if subject.account_id.is_empty() || subject.member_id.is_empty() {
            return Err(unauthorized());
        }
        if !owner_binding(db, id, &subject)? {
            return Ok(None);
        }
        receipt(db, id, &operation, &subject, None)
    })
}

pub(crate) fn assign(
    store: &Store,
    authority: Arc<dyn OwnerAuthority>,
    request: AssignmentRequest,
) -> Result<AssignmentOutcome, OfficeError> {
    let time = crate::ulid::time_ms(&request.operation_id)
        .ok_or(OfficeError::Invalid("invalid operation identity"))?;
    let request = Arc::new(request);
    let intent = request.clone();
    let check = authority.clone();
    // Read-only retry remains possible at journal exhaustion. The transaction
    // repeats this lookup at its own barrier to serialize concurrent callers.
    if let Some(result) = store.selected_read(move |db, id| {
        let owner = owner(db, id, &*check)?;
        let hash = fingerprint(id, &intent)?;
        receipt(db, id, &intent.operation_id, &owner, Some(&hash))
    })? {
        return Ok(result);
    }
    store.selected_transaction(move|db,id| {
        let owner=owner(db,id,&*authority)?;
        let hash=fingerprint(id,&request)?;
        if let Some(result)=receipt(db,id,&request.operation_id,&owner,Some(&hash))? {return Ok(result);}
        let now=now(db)?;
        const DAY:u64=86_400_000;
        if time>now.saturating_add(300_000) || now.saturating_sub(time)>29*DAY {return Err(OfficeError::Invalid("operation outside retry window"));}
        let result=decide(db,id,&request,now)?;
        // The operation's ULID time owns the window, as in legacy work and
        // gateway v1. Late admission must not extend receipt replay by 29 days.
        let expires=time.checked_add(30*DAY).and_then(|v|i64::try_from(v).ok()).ok_or_else(invalid)?;
        let bytes=serde_json::to_vec(&result).map_err(storage)?;
        db.execute("INSERT INTO selected_receipts VALUES(?1,?2,?3,?4,'owner',NULL,'task.set_assignment',1,?5,?6,?7)",params![id.organization_id,request.operation_id,id.instance_id,owner.account_id,hash,bytes,expires]).map_err(storage)?;
        let protected=serde_json::to_vec(&serde_json::json!({"receipt_version":1,"instance_id":id.instance_id,"organization_id":id.organization_id,"office_id":id.office_id,"account_id":owner.account_id,"operation_id":request.operation_id,"method":"task.set_assignment","command_version":1,"fingerprint":hash,"result":result,"expires_at_ms":expires})).map_err(storage)?;
        put_artifact(db,"protected",&format!("offices/{}/receipts/{}.json",id.office_id,request.operation_id),&protected,true)?;
        db.execute("INSERT INTO work_clock VALUES(1,?1) ON CONFLICT(id) DO UPDATE SET high_water_ms=MAX(high_water_ms,excluded.high_water_ms)",[now as i64]).map_err(storage)?;
        Ok(result)
    })
}

fn fingerprint(id: &AcceptanceIdentity, r: &AssignmentRequest) -> Result<String, OfficeError> {
    assignment::assignment_fingerprint(
        AssignmentScope {
            instance_id: &id.instance_id,
            organization_id: &id.organization_id,
            office_id: &id.office_id,
            task_id: &r.task_id,
        },
        &r.command,
    )
    .map_err(|_| OfficeError::Invalid("invalid assignment intent"))
}

fn rejection(code: &str) -> AssignmentOutcome {
    AssignmentOutcome::Rejected {
        code: code.into(),
        current: None,
    }
}

fn decide(
    db: &Connection,
    id: &AcceptanceIdentity,
    r: &AssignmentRequest,
    now: u64,
) -> Result<AssignmentOutcome, OfficeError> {
    let task:Option<(Vec<u8>,String,i64)>=db.query_row("SELECT t.markdown,t.canonical_path,a.origin_generation FROM selected_tasks t JOIN selected_adoptions a USING(organization_id,task_id) WHERE t.organization_id=?1 AND t.task_id=?2 AND a.writer_epoch=?3",params![id.organization_id,r.task_id,id.writer_epoch],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?))).optional().map_err(storage)?;
    let Some((source, path, generation)) = task else {
        return Ok(rejection("not_found"));
    };
    let source = String::from_utf8(source).map_err(|_| invalid())?;
    let snapshot = TaskSnapshot::parse(&id.organization_id, &source).map_err(storage)?;
    if snapshot.immutable().task_id != r.task_id {
        return Err(invalid());
    }
    let directory:String=db.query_row("SELECT d.data FROM selected_directory d JOIN selected_directory_head h USING(organization_id,generation) WHERE d.organization_id=?1",[&id.organization_id],|row|row.get(0)).map_err(storage)?;
    // Lossless JSON remains retained. Only the required target facts are read.
    let directory = codec::parse(directory.as_bytes()).map_err(storage)?;
    let position = directory_position(&directory)?;
    let (team, employee) = desired_targets(&directory, &r.command);
    let plan = match assignment::plan_assignment(
        &snapshot.assignment_state(),
        &r.command,
        DesiredTargetFacts {
            team: team.as_ref(),
            employee: employee.as_ref(),
        },
    ) {
        Ok(plan) => plan,
        Err(assignment::AssignmentError::RevisionConflict) => {
            return Ok(AssignmentOutcome::Rejected {
                code: "conflict".into(),
                current: Some(org_proto::selected_tasks::CurrentAssignment {
                    organization_id: id.organization_id.clone(),
                    office_id: id.office_id.clone(),
                    task_id: r.task_id.clone(),
                    revision: snapshot.revision(),
                    assigned_team_id: snapshot.assignment().assigned_team_id().map(str::to_owned),
                    assignee_id: snapshot.assignment().assignee_id().map(str::to_owned),
                }),
            })
        }
        Err(assignment::AssignmentError::RevisionOverflow) => {
            return Ok(rejection("revision_exhausted"))
        }
        Err(_) => return Ok(rejection("invalid_assignment")),
    };
    let incarnation = origin::stored_incarnation(db)?.ok_or_else(invalid)?;
    if origin::high_water(db, &incarnation)? == i64::MAX as u64 {
        return Ok(rejection("origin_exhausted"));
    }
    let at = crate::time::format_epoch_ms(now);
    let updated = snapshot
        .rewrite_assignment(&plan, &at, Updater::Owner)
        .map_err(storage)?;
    let change =
        codec::parse(&serde_json::to_vec(plan.change()).map_err(storage)?).map_err(storage)?;
    let (event, event_hash) = assignment_event(id, r, plan.next_revision(), at, change)?;
    db.execute("UPDATE selected_tasks SET revision=?1,assigned_team_id=?2,assignee_id=?3,markdown=?4,payload_sha256=?5 WHERE organization_id=?6 AND task_id=?7",params![plan.next_revision(),r.command.assigned_team_id,r.command.assignee_id,updated.as_bytes(),codec::sha256(updated.as_bytes()),id.organization_id,r.task_id]).map_err(storage)?;
    db.execute(
        "INSERT INTO selected_events VALUES(?1,?2,?3,2,?4,?5,?6)",
        params![
            id.organization_id,
            r.task_id,
            plan.next_revision(),
            codec::RESOURCE_PROFILE,
            event_hash,
            event
        ],
    )
    .map_err(storage)?;
    put_artifact(db, "public", &path, updated.as_bytes(), false)?;
    put_artifact(
        db,
        "public",
        &format!(
            "work-history/tasks/{}/{}.json",
            r.task_id,
            plan.next_revision()
        ),
        &event,
        true,
    )?;
    let refs = serde_json::to_string(&vec![org_proto::work::EventRef {
        target_id: r.task_id.clone(),
        version: plan.next_revision() as u64,
    }])
    .map_err(storage)?;
    let feed: i64 = db
        .query_row(
            "INSERT INTO work_feed(change_id,targets) VALUES(?1,?2) RETURNING seq",
            params![r.operation_id, refs],
            |row| row.get(0),
        )
        .map_err(storage)?;
    let range = origin::append_selected(
        db,
        position,
        &[origin::OriginRef {
            target_id: r.task_id.clone(),
            generation: generation as u64,
            version: plan.next_revision() as u64,
        }],
        feed as u64,
        now,
    )?;
    if range.is_empty() {
        return Err(invalid());
    }
    Ok(AssignmentOutcome::Completed {
        organization_id: id.organization_id.clone(),
        office_id: id.office_id.clone(),
        task_id: r.task_id.clone(),
        revision: plan.next_revision(),
        assigned_team_id: r.command.assigned_team_id.clone(),
        assignee_id: r.command.assignee_id.clone(),
        event_sha256: event_hash,
    })
}

pub(crate) fn put_artifact(
    db: &Connection,
    namespace: &str,
    path: &str,
    content: &[u8],
    immutable: bool,
) -> Result<(), OfficeError> {
    db.execute("INSERT INTO selected_artifacts VALUES(?1,?2,?3,?4) ON CONFLICT(namespace,path) DO UPDATE SET content=excluded.content WHERE selected_artifacts.immutable=0 AND excluded.immutable=0",params![namespace,path,content,immutable]).map_err(storage)?;
    // An ignored immutable conflict is never treated as successful publication.
    let actual: Vec<u8> = db
        .query_row(
            "SELECT content FROM selected_artifacts WHERE namespace=?1 AND path=?2",
            params![namespace, path],
            |r| r.get(0),
        )
        .map_err(storage)?;
    if actual != content {
        return Err(invalid());
    }
    Ok(())
}

#[cfg(test)]
mod tests;

fn directory_position(directory: &Json) -> Result<org_proto::Position, OfficeError> {
    let position = directory.get("source_position").ok_or_else(invalid)?;
    let counter = |key| match position.get(key) {
        Some(Json::Number(n)) if n.as_str() == "0" => Ok(0),
        Some(n) => n.positive_i64().map(|n| n as u64).ok_or_else(invalid),
        None => Err(invalid()),
    };
    let position = org_proto::Position {
        epoch: counter("epoch")?,
        seq: counter("seq")?,
    };
    Ok(position)
}

fn desired_targets(
    directory: &Json,
    command: &AssignmentCommand,
) -> (Option<TeamFact>, Option<EmployeeFact>) {
    let team = command
        .assigned_team_id
        .as_deref()
        .and_then(|key| directory.get("teams")?.get(key))
        .map(|fact| TeamFact {
            organization_id: fact
                .get("organization_id")
                .and_then(Json::as_str)
                .unwrap_or("")
                .into(),
            id: fact.get("id").and_then(Json::as_str).unwrap_or("").into(),
            active: fact.get("status").and_then(Json::as_str) == Some("active"),
        });
    let employee = command
        .assignee_id
        .as_deref()
        .and_then(|key| directory.get("employees")?.get(key))
        .map(|fact| EmployeeFact {
            organization_id: fact
                .get("organization_id")
                .and_then(Json::as_str)
                .unwrap_or("")
                .into(),
            id: fact.get("id").and_then(Json::as_str).unwrap_or("").into(),
            active: fact.get("status").and_then(Json::as_str) == Some("active"),
        });
    (team, employee)
}

fn assignment_event(
    id: &AcceptanceIdentity,
    r: &AssignmentRequest,
    revision: i64,
    at: String,
    change: Json,
) -> Result<(Vec<u8>, String), OfficeError> {
    let event = history::encode(
        EventFields {
            organization_id: id.organization_id.clone(),
            task_id: r.task_id.clone(),
            revision,
            occurred_at: Some(at),
            actor_kind: "owner".into(),
            actor_id: None,
            operation_id: Some(r.operation_id.clone()),
            change,
            source_metadata: BTreeMap::new(),
            extensions: BTreeMap::new(),
        },
        Admission::New,
    )
    .map_err(storage)?;
    let verified = history::verify(
        &event,
        history::ExpectedIdentity {
            organization_id: &id.organization_id,
            task_id: &r.task_id,
            filename: &format!("{}.json", revision),
        },
        Admission::New,
    )
    .map_err(storage)?;
    let event_hash = verified
        .value()
        .get("payload_sha256")
        .and_then(Json::as_str)
        .ok_or_else(invalid)?
        .to_owned();
    Ok((event, event_hash))
}
