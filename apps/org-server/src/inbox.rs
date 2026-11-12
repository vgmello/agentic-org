//! Consumer-owned source contract for the Inbox composition root. Domains never depend on
//! this module. Each source returns counts and items from one read view, or fails as a unit.
use org_proto::inbox::{InboxCounts, InboxItem, InboxKind, InboxPosition, InboxState};
use std::sync::atomic::{AtomicU64, Ordering};

mod list;
mod org_source;

/// HQ's real source registration and the clock also used by hint delivery.
pub(crate) struct Hq {
    pub(crate) sources: Vec<std::sync::Arc<dyn Source>>,
    pub(crate) clock: Clock,
}

impl Hq {
    pub(crate) fn for_office(office: std::sync::Arc<org_office::Office>) -> std::io::Result<Self> {
        use std::io::Read;
        let mut epoch = [0; 16];
        std::fs::File::open("/dev/urandom")?.read_exact(&mut epoch)?;
        let office_id = office.info().office_id.clone();
        let source = org_source::OrgSource::new(
            Box::new(move || office.org_snapshot().map_err(|_| SourceFailure)),
            office_id,
        );
        Ok(Self {
            sources: vec![std::sync::Arc::new(source)],
            clock: Clock::new(format!("{:032x}", u128::from_be_bytes(epoch))),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keyset {
    pub created_at_ms: Option<u64>,
    pub reference: String,
}

impl Keyset {
    pub fn of(item: &InboxItem) -> Self {
        Self {
            created_at_ms: item.created_at_ms,
            reference: item.reference.clone(),
        }
    }
    /// Descending known times, then unknown times; refs break ties in ascending order.
    pub fn admits(&self, item: &InboxItem) -> bool {
        self.created_at_ms
            .cmp(&item.created_at_ms)
            .then_with(|| item.reference.cmp(&self.reference))
            .is_gt()
    }
}

/// A source applies the keyset and filters before its item cap. Counts ignore all filters.
pub struct Query<'a> {
    pub state: InboxState,
    pub kinds: Option<&'a [InboxKind]>,
    pub project: Option<&'a str>,
    pub team: Option<&'a str>,
    pub after: Option<&'a Keyset>,
    /// Zero requests counts only; other reads return at most 500 items per source.
    pub limit: usize,
}

pub struct SourceRead {
    pub items: Vec<InboxItem>,
    pub counts: InboxCounts,
}

#[derive(Debug)]
pub struct SourceFailure;

/// An atomic read failure omits both this source's items and its counts.
pub trait Source: Send + Sync {
    fn name(&self) -> &'static str;
    fn read(&self, query: &Query<'_>) -> Result<SourceRead, SourceFailure>;
}

pub struct Totals {
    pub counts: InboxCounts,
    pub degraded: Vec<String>,
}

pub struct Read {
    pub items: Vec<InboxItem>,
    pub totals: Totals,
}

/// Capture the position before reading any source, including one that commits during a read.
pub fn read(clock: &Clock, sources: &[&dyn Source], query: &Query<'_>) -> (InboxPosition, Read) {
    let position = clock.as_of();
    (position, collect(sources, query))
}

/// The same atomic source reads and aggregation power list and the count-only hint path.
fn collect(sources: &[&dyn Source], query: &Query<'_>) -> Read {
    let mut result = Read {
        items: vec![],
        totals: Totals {
            counts: InboxCounts::default(),
            degraded: vec![],
        },
    };
    for source in sources {
        let read = source.read(query).ok().and_then(|read| {
            let a = &result.totals.counts;
            let b = &read.counts;
            Some((
                read.items,
                InboxCounts {
                    approvals: a.approvals.checked_add(b.approvals)?,
                    questions: a.questions.checked_add(b.questions)?,
                    decisions: a.decisions.checked_add(b.decisions)?,
                    problems: a.problems.checked_add(b.problems)?,
                    total: a.total.checked_add(b.total)?,
                },
            ))
        });
        if let Some((items, counts)) = read {
            result.items.extend(items);
            result.totals.counts = counts;
        } else {
            result.totals.degraded.push(source.name().to_owned());
        }
    }
    result.items.sort_by(|a, b| {
        b.created_at_ms
            .cmp(&a.created_at_ms)
            .then_with(|| a.reference.cmp(&b.reference))
    });
    result.items.truncate(query.limit);
    for item in &mut result.items {
        item.bound_text();
    }
    result
}

/// Recompute unfiltered counts once per relevant committed change. No item read is needed.
pub fn recount(sources: &[&dyn Source]) -> Totals {
    collect(
        sources,
        &Query {
            state: InboxState::Open,
            kinds: None,
            project: None,
            team: None,
            after: None,
            limit: 0,
        },
    )
    .totals
}

/// One clock shared by list and hint; sample `as_of` before the first source read.
pub struct Clock {
    epoch: String,
    seq: AtomicU64,
}

impl Clock {
    pub fn new(epoch: String) -> Self {
        Self {
            epoch,
            seq: AtomicU64::new(0),
        }
    }
    pub fn as_of(&self) -> InboxPosition {
        InboxPosition {
            epoch: self.epoch.clone(),
            seq: self.seq.load(Ordering::Acquire),
        }
    }
    /// a2 calls this after a committed relevant change, independently of emission throttling.
    pub fn changed(&self) {
        self.seq
            .try_update(Ordering::AcqRel, Ordering::Acquire, |n| n.checked_add(1))
            .expect("inbox sequence exhausted");
    }
}

#[cfg(test)]
mod tests;
