//! Authorized, coherent selected reads on the existing office writer barrier.
use super::{service, AcceptanceIdentity, OwnerAuthority, TaskSnapshot};
use crate::{store::Store, OfficeError};
use org_proto::selected_tasks::{
    read::{
        self as wire, Card, ContentKind, ContentRef, Query, Queue, Result as Answer, Target,
        TargetKind,
    },
    MAX_CHUNK_BYTES,
};
use org_work::selected_codec::{self as codec, Json};
use rusqlite::{params, Connection, OptionalExtension};
use std::sync::Arc;

fn storage(e: impl std::error::Error + Send + Sync + 'static) -> OfficeError {
    OfficeError::storage(e)
}
fn invalid() -> OfficeError {
    OfficeError::Invalid("selected read rejected")
}
fn limit(n: u16) -> Result<usize, OfficeError> {
    if n == 0 || n > wire::PAGE_MAX {
        Err(invalid())
    } else {
        Ok(usize::from(n))
    }
}
fn preview(s: &str) -> String {
    let mut chars = s.chars();
    let mut out: String = chars.by_ref().take(256).collect();
    if chars.next().is_some() {
        out.push('…');
    }
    out
}
fn compact_id(s: &str) -> Result<(), OfficeError> {
    if s.is_empty() || s.len() > 255 {
        Err(invalid())
    } else {
        Ok(())
    }
}
fn card(org: &str, bytes: Vec<u8>, digest: String) -> Result<Card<i64>, OfficeError> {
    if codec::sha256(&bytes) != digest {
        return Err(invalid());
    }
    let source = std::str::from_utf8(&bytes).map_err(storage)?;
    let snapshot = TaskSnapshot::parse(org, source).map_err(storage)?;
    let facts = snapshot.immutable();
    let pair = snapshot.assignment();
    for id in [facts.task_id.as_str(), facts.team_id.as_str()]
        .into_iter()
        .chain(facts.project_id.iter().map(String::as_str))
        .chain(pair.assigned_team_id())
        .chain(pair.assignee_id())
    {
        compact_id(id)?;
    }
    Ok(Card {
        task_id: facts.task_id.clone(),
        revision: snapshot.revision(),
        accountable_team_id: facts.team_id.clone(),
        project_id: facts.project_id.clone(),
        number: facts.number.map(|n| n.to_string()),
        title_preview: preview(&facts.title),
        status: facts.status.clone(),
        assigned_team_id: pair.assigned_team_id().map(str::to_owned),
        assignee_id: pair.assignee_id().map(str::to_owned),
        snapshot: ContentRef {
            task_id: facts.task_id.clone(),
            revision: snapshot.revision(),
            kind: ContentKind::Snapshot,
            sha256: digest,
            byte_length: bytes.len() as u64,
        },
    })
}
fn task(db: &Connection, id: &AcceptanceIdentity, task: &str) -> Result<Card<i64>, OfficeError> {
    let row=db.query_row("SELECT markdown,payload_sha256 FROM selected_tasks WHERE organization_id=?1 AND task_id=?2",params![id.organization_id,task],|r|Ok((r.get::<_,Vec<u8>>(0)?,r.get::<_,String>(1)?))).optional().map_err(storage)?.ok_or(OfficeError::NotFound)?;
    card(&id.organization_id, row.0, row.1)
}
fn coverage(db: &Connection, id: &AcceptanceIdentity, task: &str) -> Result<String, OfficeError> {
    let source:String=db.query_row("SELECT source_metadata FROM selected_adoptions WHERE organization_id=?1 AND task_id=?2",params![id.organization_id,task],|r|r.get(0)).map_err(storage)?;
    let source = codec::parse(source.as_bytes()).map_err(storage)?;
    Ok(
        match source.get("history_coverage").and_then(Json::as_str) {
            Some("complete") => "complete",
            Some("snapshot_baseline") => "snapshot_baseline",
            Some("known_gaps") => "known_gaps",
            _ => "unknown",
        }
        .into(),
    )
}
fn generation(db: &Connection, id: &AcceptanceIdentity) -> Result<i64, OfficeError> {
    db.query_row(
        "SELECT generation FROM selected_directory_head WHERE organization_id=?1",
        [&id.organization_id],
        |r| r.get(0),
    )
    .optional()
    .map_err(storage)?
    .ok_or(OfficeError::SnapshotPending)
}
pub(crate) fn ready(store: &Store) -> Result<bool, OfficeError> {
    store.selected_read(|db,id| {
        let owner:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM selected_owner_authority WHERE organization_id=?1 AND active=1)",[&id.organization_id],|r|r.get(0)).map_err(storage)?;
        Ok(owner && generation(db,id).is_ok())
    })
}
pub(crate) fn adoption_ready(store: &Store) -> Result<bool, OfficeError> {
    // selected_read verifies the writer lease and frozen journal prefix first.
    // Initial public directory import cannot depend on an existing directory.
    store.selected_read(|db, id| {
        db.query_row(
            "SELECT EXISTS(SELECT 1 FROM selected_owner_authority WHERE organization_id=?1 AND active=1)",
            [&id.organization_id], |r| r.get(0),
        ).map_err(storage)
    })
}
pub(crate) fn read(
    store: &Store,
    authority: Arc<dyn OwnerAuthority>,
    query: Query,
) -> Result<Answer<i64>, OfficeError> {
    store.selected_read(move|db,id| {
        service::owner(db,id,&*authority)?;
        let generation=generation(db,id)?;
        match query {
            Query::Status => Ok(Answer::Status {directory_generation:generation}),
            Query::Detail {task_id} => Ok(Answer::Detail {task:task(db,id,&task_id)?,history_coverage:coverage(db,id,&task_id)?}),
            Query::List {after,queue,limit:count} => {
                let count=limit(count)?;
                let (kind,value)=match queue {Queue::All=>("all",String::new()),Queue::Owned {team_id}=>("owned",team_id),Queue::AssignedTeam {team_id}=>("team",team_id),Queue::AssignedEmployee {employee_id}=>("employee",employee_id)};
                let mut query=db.prepare("SELECT markdown,payload_sha256 FROM selected_tasks WHERE organization_id=?1 AND task_id>?2 AND (?3='all' OR (?3='owned' AND team_id=?4) OR (?3='team' AND assigned_team_id=?4) OR (?3='employee' AND assignee_id=?4)) ORDER BY task_id LIMIT ?5").map_err(storage)?;
                let mut rows=query.query(params![id.organization_id,after.unwrap_or_default(),kind,value,(count+1) as i64]).map_err(storage)?;
                let mut items=Vec::new();let mut more=false;let mut encoded=0usize;
                while let Some(row)=rows.next().map_err(storage)? {
                    if items.len()==count {more=true;break;}
                    let card=card(&id.organization_id,row.get(0).map_err(storage)?,row.get(1).map_err(storage)?)?;
                    // Page bytes as well as row count are bounded. Retain the
                    // complete status/identities; never truncate source fields.
                    // Reserve ample room for native/browser wrappers/revisions.
                    let bytes=serde_json::to_vec(&card).map_err(storage)?.len().checked_add(64).ok_or_else(invalid)?;
                    if encoded.checked_add(bytes).is_none_or(|n|n>org_proto::MAX_FRAME_BYTES/2) {
                        if items.is_empty(){return Err(invalid());}more=true;break;
                    }
                    encoded+=bytes;items.push(card);
                }
                let next=more.then(||items.last().expect("nonzero page").task_id.clone());
                Ok(Answer::List {items,next})
            }
            Query::History {task_id,after,limit:count} => {
                let count=limit(count)?;task(db,id,&task_id)?;
                let mut query=db.prepare("SELECT revision,event_bytes FROM selected_events WHERE organization_id=?1 AND task_id=?2 AND revision>?3 ORDER BY revision LIMIT ?4").map_err(storage)?;
                // Transport content identities hash the complete retained bytes.
                // payload_sha256 is the event codec checksum (which omits the
                // checksum property); it must never identify the full document.
                // Hash one bounded event at a time, retaining only references.
                let events=query.query_map(params![id.organization_id,task_id,after.map(|r|r.get()).unwrap_or(0),(count+1) as i64],|r| {
                    let bytes:Vec<u8>=r.get(1)?;
                    Ok(ContentRef {task_id:task_id.clone(),revision:r.get(0)?,kind:ContentKind::Event,sha256:codec::sha256(&bytes),byte_length:bytes.len() as u64})
                }).map_err(storage)?.collect::<Result<Vec<_>,_>>().map_err(storage)?;
                let mut events=events;let more=events.len()>count;events.truncate(count);
                let next=more.then(||events.last().expect("nonzero page").revision);
                Ok(Answer::History {events,next,coverage:coverage(db,id,&task_id)?})
            }
            Query::Targets {task_id,target_kind,after,limit:count} => {
                let count=limit(count)?;task(db,id,&task_id)?;
                let bytes:String=db.query_row("SELECT data FROM selected_directory WHERE organization_id=?1 AND generation=?2",params![id.organization_id,generation],|r|r.get(0)).map_err(storage)?;
                let source=codec::parse(bytes.as_bytes()).map_err(storage)?;
                let key=if target_kind==TargetKind::Team {"teams"}else{"employees"};
                let Some(Json::Object(facts))=source.get(key) else{return Err(invalid());};
                let mut items=Vec::new();let mut more=false;
                for (key,fact) in facts {
                    if after.as_ref().is_some_and(|after|key<=after) {continue;}
                    if fact.get("status").and_then(Json::as_str)!=Some("active") {continue;}
                    if items.len()==count {more=true;break;}
                    compact_id(key)?;
                    items.push(Target {id:key.clone(),name_preview:preview(fact.get("name").and_then(Json::as_str).ok_or_else(invalid)?),active:true});
                }
                let next=more.then(||items.last().expect("nonzero page").id.clone());
                Ok(Answer::Targets {directory_generation:generation,items,next})
            }
            Query::Chunk {task_id,reference,offset,limit} => {
                if limit==0 || limit as usize>MAX_CHUNK_BYTES || reference.task_id!=task_id || reference.revision<=0 || offset>reference.byte_length {return Err(invalid());}
                task(db,id,&task_id)?;
                let (table,column)=match reference.kind {ContentKind::Snapshot=>("selected_tasks","markdown"),ContentKind::Event=>("selected_events","event_bytes")};
                let sql=format!("SELECT {column} FROM {table} WHERE organization_id=?1 AND task_id=?2 AND revision=?3 AND length({column})=?4");
                let source:Vec<u8>=db.query_row(&sql,params![id.organization_id,task_id,reference.revision,i64::try_from(reference.byte_length).map_err(|_|invalid())?],|r|r.get(0)).optional().map_err(storage)?.ok_or(OfficeError::NotFound)?;
                if codec::sha256(&source)!=reference.sha256 {return Err(OfficeError::NotFound);}
                let start=usize::try_from(offset).map_err(|_|invalid())?;
                let end=start.saturating_add(limit as usize).min(source.len());
                let bytes=&source[start..end];
                let next=offset.checked_add(bytes.len() as u64).filter(|n|*n<reference.byte_length);
                let hex=bytes.iter().map(|b|format!("{b:02x}")).collect();
                Ok(Answer::Chunk {reference,offset,hex,next})
            }
        }
    })
}
