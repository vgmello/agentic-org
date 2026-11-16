//! Decode query entries before map construction. Escaped duplicates, unknown
//! fields, noncanonical counters and lossy revision conversions never dispatch.
use super::*;
fn hex(c: u8) -> Result<u8, ApiReply> {
    match c {
        b'0'..=b'9' => Ok(c - b'0'),
        b'a'..=b'f' => Ok(c - b'a' + 10),
        b'A'..=b'F' => Ok(c - b'A' + 10),
        _ => Err(invalid()),
    }
}
fn decoded(s: &str) -> Result<String, ApiReply> {
    let mut input = s.bytes();
    let mut out = Vec::with_capacity(s.len());
    while let Some(c) = input.next() {
        let c = match c {
            b'%' => {
                hex(input.next().ok_or_else(invalid)?)? * 16
                    + hex(input.next().ok_or_else(invalid)?)?
            }
            b'+' => b' ',
            c => c,
        };
        if c.is_ascii_control() {
            return Err(invalid());
        }
        out.push(c);
    }
    String::from_utf8(out).map_err(|_| invalid())
}
pub(super) fn decode(s: &str) -> Result<BTreeMap<String, String>, ApiReply> {
    if s.len() > 16 * 1024 {
        return Err(invalid());
    }
    let mut out = BTreeMap::new();
    if s.is_empty() {
        return Ok(out);
    }
    for part in s.split('&') {
        let (key, value) = part.split_once('=').ok_or_else(invalid)?;
        let key = decoded(key)?;
        let value = decoded(value)?;
        if key.is_empty() || out.len() >= 16 || out.insert(key, value).is_some() {
            return Err(invalid());
        }
    }
    Ok(out)
}
pub(super) fn identity(s: &str) -> Result<&str, ApiReply> {
    if s.is_empty() || s.len() > 255 || s.chars().any(char::is_control) {
        Err(invalid())
    } else {
        Ok(s)
    }
}
fn take(q: &mut BTreeMap<String, String>, key: &str) -> Result<String, ApiReply> {
    q.remove(key).ok_or_else(invalid)
}
fn counter(s: &str) -> Result<u64, ApiReply> {
    if s.is_empty()
        || s.len() > 16
        || !s.bytes().all(|b| b.is_ascii_digit())
        || (s.len() > 1 && s.starts_with('0'))
    {
        return Err(invalid());
    }
    s.parse::<u64>()
        .ok()
        .filter(|n| *n <= org_proto::gateway::MAX_SAFE_INTEGER)
        .ok_or_else(invalid)
}
fn revision(s: &str) -> Result<Revision, ApiReply> {
    if s.is_empty() || s.starts_with('0') || s.len() > 19 || !s.bytes().all(|b| b.is_ascii_digit())
    {
        return Err(invalid());
    }
    Revision::new(s.parse().map_err(|_| invalid())?).map_err(|_| invalid())
}
fn after(q: &mut BTreeMap<String, String>) -> Result<Option<String>, ApiReply> {
    q.remove("after")
        .map(|v| identity(&v).map(str::to_owned))
        .transpose()
}
fn limit(q: &mut BTreeMap<String, String>) -> Result<u16, ApiReply> {
    let n = counter(&take(q, "limit")?)?;
    if !(1..=u64::from(task::read::PAGE_MAX)).contains(&n) {
        return Err(invalid());
    }
    Ok(n as u16)
}
pub(super) fn scope(
    q: &mut BTreeMap<String, String>,
    organization: Option<&str>,
) -> Result<OfficeScope, ApiReply> {
    let instance_id = take(q, "instance_id")?;
    let office_id = take(q, "office_id")?;
    if org_proto::ulid_time_ms(&instance_id).is_none() {
        return Err(invalid());
    }
    identity(&office_id)?;
    let org_id = match organization {
        Some(id) => identity(id)?.into(),
        None => {
            let id = take(q, "org_id")?;
            identity(&id)?;
            id
        }
    };
    Ok(OfficeScope {
        kind: task::OfficeKind::Office,
        instance_id,
        org_id,
        office_id,
    })
}
pub(super) fn read(path: &[String], q: &mut BTreeMap<String, String>) -> Result<Query, ApiReply> {
    if path.len() == 5 {
        return match take(q, "kind")?.as_str() {
            "status" => Ok(Query::Status),
            "list" => {
                let queue = read_queue(q)?;
                Ok(Query::List {
                    after: after(q)?,
                    queue,
                    limit: limit(q)?,
                })
            }
            _ => Err(invalid()),
        };
    }
    let task_id = identity(path.get(5).ok_or_else(invalid)?)?.into();
    if path.len() == 6 {
        return Ok(Query::Detail { task_id });
    }
    if path.len() != 7 {
        return Err(invalid());
    }
    match path[6].as_str() {
        "history" => Ok(Query::History {
            task_id,
            after: q.remove("after").map(|s| revision(&s)).transpose()?,
            limit: limit(q)?,
        }),
        "assignment-targets" => Ok(Query::Targets {
            task_id,
            target_kind: match take(q, "target_kind")?.as_str() {
                "team" => TargetKind::Team,
                "employee" => TargetKind::Employee,
                _ => return Err(invalid()),
            },
            after: after(q)?,
            limit: limit(q)?,
        }),
        "content" => read_chunk(task_id, q),
        _ => Err(missing()),
    }
}

fn read_queue(q: &mut BTreeMap<String, String>) -> Result<Queue, ApiReply> {
    let queue = match take(q, "queue")?.as_str() {
        "all" => Queue::All,
        "owned" => Queue::Owned {
            team_id: take(q, "target_id")?,
        },
        "assigned_team" => Queue::AssignedTeam {
            team_id: take(q, "target_id")?,
        },
        "assigned_employee" => Queue::AssignedEmployee {
            employee_id: take(q, "target_id")?,
        },
        _ => return Err(invalid()),
    };
    match &queue {
        Queue::All => {}
        Queue::Owned { team_id } | Queue::AssignedTeam { team_id } => {
            identity(team_id)?;
        }
        Queue::AssignedEmployee { employee_id } => {
            identity(employee_id)?;
        }
    }
    Ok(queue)
}

fn read_chunk(task_id: String, q: &mut BTreeMap<String, String>) -> Result<Query, ApiReply> {
    let revision = revision(&take(q, "revision")?)?.get();
    let kind = match take(q, "kind")?.as_str() {
        "snapshot" => ContentKind::Snapshot,
        "event" => ContentKind::Event,
        _ => return Err(invalid()),
    };
    let sha256 = take(q, "sha256")?;
    if sha256.len() != 64
        || !sha256
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(invalid());
    }
    let byte_length = counter(&take(q, "byte_length")?)?;
    let offset = counter(&take(q, "offset")?)?;
    let limit = counter(&take(q, "limit")?)?;
    if byte_length > 16 * 1024 * 1024
        || offset > byte_length
        || limit == 0
        || limit > task::MAX_CHUNK_BYTES as u64
    {
        return Err(invalid());
    }
    let reference = ContentRef {
        task_id: task_id.clone(),
        revision,
        kind,
        sha256,
        byte_length,
    };
    Ok(Query::Chunk {
        task_id,
        reference,
        offset,
        limit: limit as u32,
    })
}
