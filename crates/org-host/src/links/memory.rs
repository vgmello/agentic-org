//! In-memory `OneTimeLinks` adapter. It proves the storage seam (ADR-0018 ENG-2)
//! and runs the same contract suite as the SQLite registry.
use std::sync::Mutex;

use super::{
    redeem_rule, Error, LinkDigest, LinkKind, NewLink, OneTimeLinks, Pending, Refused, StoreFault,
    Subject,
};

struct Row {
    link_id: String,
    kind: LinkKind,
    subject: Subject,
    digest: LinkDigest,
    pending: Pending,
    state: &'static str,
}

pub(crate) struct MemoryLinks {
    instance_id: String,
    rows: Mutex<Vec<Row>>,
}
impl MemoryLinks {
    pub(crate) fn new(instance_id: &str) -> Self {
        Self {
            instance_id: instance_id.to_owned(),
            rows: Mutex::new(Vec::new()),
        }
    }
}

impl OneTimeLinks for MemoryLinks {
    fn instance_id(&self) -> &str {
        &self.instance_id
    }
    fn replace_pending(&self, link: NewLink<'_>) -> Result<(), Error> {
        let mut rows = self
            .rows
            .lock()
            .map_err(|_| Error::Store(StoreFault::Poisoned))?;
        if conflicts(&rows, &link) {
            return Err(Error::Store(StoreFault::Constraint));
        }
        for row in rows.iter_mut() {
            if row.kind == link.kind && row.subject == *link.subject && row.state == "pending" {
                row.state = "cancelled";
            }
        }
        push(&mut rows, link)?;
        Ok(())
    }
    fn insert_first(&self, link: NewLink<'_>) -> Result<bool, Error> {
        let mut rows = self
            .rows
            .lock()
            .map_err(|_| Error::Store(StoreFault::Poisoned))?;
        if rows
            .iter()
            .any(|r| r.kind == link.kind && r.subject == *link.subject)
        {
            return Ok(false);
        }
        push(&mut rows, link)?;
        Ok(true)
    }
    fn redeem(
        &self,
        kind: LinkKind,
        digest: LinkDigest,
        now_ms: i64,
    ) -> Result<Result<String, Refused>, Error> {
        let mut rows = self
            .rows
            .lock()
            .map_err(|_| Error::Store(StoreFault::Poisoned))?;
        let Some(row) = rows
            .iter_mut()
            .find(|r| r.kind == kind && r.digest == digest)
        else {
            return Ok(Err(Refused::Unknown));
        };
        if let Err(refused) = redeem_rule(row.state, row.pending.expires_at_ms, now_ms) {
            return Ok(Err(refused));
        }
        row.state = "used";
        Ok(Ok(row.link_id.clone()))
    }
    fn pending_row(&self, kind: LinkKind, subject: &Subject) -> Result<Option<Pending>, Error> {
        let rows = self
            .rows
            .lock()
            .map_err(|_| Error::Store(StoreFault::Poisoned))?;
        Ok(rows
            .iter()
            .find(|r| r.kind == kind && r.subject == *subject && r.state == "pending")
            .map(|r| r.pending.clone()))
    }
}

fn conflicts(rows: &[Row], link: &NewLink<'_>) -> bool {
    rows.iter()
        .any(|r| r.link_id == link.link_id || r.digest == link.digest)
}

/// Appends a pending row, refusing a duplicate link ID or digest as the
/// SQLite constraints do.
fn push(rows: &mut Vec<Row>, link: NewLink<'_>) -> Result<(), Error> {
    if conflicts(rows, &link) {
        return Err(Error::Store(StoreFault::Constraint));
    }
    rows.push(Row {
        link_id: link.link_id.to_owned(),
        kind: link.kind,
        subject: link.subject.clone(),
        digest: link.digest,
        pending: Pending {
            link_id: link.link_id.to_owned(),
            issued_at_ms: link.issued_at_ms,
            expires_at_ms: link.expires_at_ms,
        },
        state: "pending",
    });
    Ok(())
}
