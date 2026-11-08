//! In-memory implementations of every port, for tests here and in the office (feature
//! `testkit`): [`MemOrg`] ([`OrgView`]), [`MemStore`] ([`WorkStore`], [`WorkReads`]; its
//! transactions are [`WorkTxn`]s over a copy that is kept only if the body succeeds) and a
//! settable [`TestClock`].
//!
//! [`MemStore`] can also run a request's body twice, discarding the first attempt, as the
//! office's writer does when SQLite ends a batch ([`MemStore::rerun_next`]), and fail a chosen
//! write once ([`MemStore::fail_next`]).

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use org_proto::org::{
    AgentProfile, AgentRuntime, AgentStatus, Member, MemberKind, Position, Project,
};
use org_proto::work::{EventRef, Link, LinkTarget, WorkEvent, WorkPosition};

use crate::engine::TxnOutput;
use crate::model::{link_view, StoredItem, StoredResult};
use crate::ports::{Clock, NumberScope, OrgView, StoreError, TxnFn, WorkReads, WorkStore, WorkTxn};

/// The organization, in memory.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MemOrg {
    pub owner_id: String,
    pub members: BTreeMap<String, Member>,
    pub projects: BTreeMap<String, Project>,
    /// Project id → the ids of its teams.
    pub project_teams: BTreeMap<String, Vec<String>>,
    pub position: Position,
}

impl MemOrg {
    /// An organization whose owner is `owner_id`.
    pub fn new(owner_id: &str) -> Self {
        let mut org = Self {
            owner_id: owner_id.to_owned(),
            ..Self::default()
        };
        org.members.insert(
            owner_id.to_owned(),
            Member {
                owner: true,
                ..person(owner_id)
            },
        );
        org
    }

    /// Adds a person.
    pub fn with_person(mut self, id: &str) -> Self {
        self.members.insert(id.to_owned(), person(id));
        self
    }

    /// Adds an agent in `status`.
    pub fn with_agent(mut self, id: &str, status: AgentStatus) -> Self {
        self.members.insert(
            id.to_owned(),
            Member {
                id: id.to_owned(),
                kind: MemberKind::Agent,
                name: id.to_owned(),
                contact: None,
                owner: false,
                office_id: Some("hq".to_owned()),
                agent: Some(AgentProfile {
                    role: "software engineer".into(),
                    title: "Engineer".into(),
                    focus: None,
                    runtime: AgentRuntime::ClaudeCode,
                    model: None,
                    run_mode: None,
                    schedule: None,
                    home: None,
                    messaging_name: id.to_owned(),
                    status,
                    offboarded_at: None,
                }),
                reports_to_member_id: None,
                created_at: None,
                version: 1,
            },
        );
        self
    }

    /// Adds a project with `key` and teams.
    pub fn with_project(mut self, id: &str, key: &str, teams: &[&str]) -> Self {
        let mut p = Project::new(id, format!("Project {key}"));
        p.key = key.to_owned();
        p.version = 1;
        self.projects.insert(id.to_owned(), p);
        self.project_teams.insert(
            id.to_owned(),
            teams.iter().map(|t| (*t).to_owned()).collect(),
        );
        self
    }

    /// Archives (or unarchives) a project.
    pub fn set_archived(&mut self, id: &str, archived: bool) {
        if let Some(p) = self.projects.get_mut(id) {
            p.archived = archived;
        }
    }
}

fn person(id: &str) -> Member {
    Member {
        id: id.to_owned(),
        kind: MemberKind::Person,
        name: id.to_owned(),
        contact: None,
        owner: false,
        office_id: None,
        agent: None,
        reports_to_member_id: None,
        created_at: None,
        version: 1,
    }
}

impl OrgView for MemOrg {
    fn applied_position(&self) -> Result<Position, StoreError> {
        Ok(self.position)
    }
    fn member(&self, id: &str) -> Result<Option<Member>, StoreError> {
        Ok(self.members.get(id).cloned())
    }
    fn owner_id(&self) -> Result<String, StoreError> {
        Ok(self.owner_id.clone())
    }
    fn project(&self, id: &str) -> Result<Option<Project>, StoreError> {
        Ok(self.projects.get(id).cloned())
    }
    fn project_by_key(&self, key: &str) -> Result<Option<Project>, StoreError> {
        Ok(self.projects.values().find(|p| p.key == key).cloned())
    }
    fn project_teams(&self, project_id: &str) -> Result<Vec<String>, StoreError> {
        Ok(self
            .project_teams
            .get(project_id)
            .cloned()
            .unwrap_or_default())
    }
}

/// One committed call's feed row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedRow {
    pub position: WorkPosition,
    pub change_id: Option<String>,
    pub org_position: Position,
    pub refs: Vec<EventRef>,
}

/// The work data, in memory.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MemWork {
    pub items: BTreeMap<String, StoredItem>,
    pub events: BTreeMap<String, Vec<WorkEvent>>,
    /// Scope key → the last number given.
    pub numbers: BTreeMap<String, u64>,
    pub results: BTreeMap<String, StoredResult>,
    pub feed: Vec<FeedRow>,
}

/// A write the store can be told to fail once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailPoint {
    PutItem,
    PutChangeResult,
    RecordChange,
}

/// A work store in memory. Each `transact` works on a copy and keeps it only when the body
/// returns `Ok`.
#[derive(Debug, Default)]
pub struct MemStore {
    pub work: Mutex<MemWork>,
    pub org: Mutex<MemOrg>,
    rerun: AtomicBool,
    fail: Mutex<Option<FailPoint>>,
    attempts: AtomicU64,
}

impl MemStore {
    pub fn new(org: MemOrg) -> Self {
        Self {
            org: Mutex::new(org),
            ..Self::default()
        }
    }

    /// The next `transact` runs its body, throws that attempt away, and runs it again.
    pub fn rerun_next(&self) {
        self.rerun.store(true, Ordering::SeqCst);
    }

    /// The next time this write is attempted, it fails with a storage error.
    pub fn fail_next(&self, point: FailPoint) {
        *self.fail.lock().expect("lock") = Some(point);
    }

    /// How many times a body has run.
    pub fn attempts(&self) -> u64 {
        self.attempts.load(Ordering::SeqCst)
    }

    /// A copy of the work data.
    pub fn snapshot(&self) -> MemWork {
        self.work.lock().expect("lock").clone()
    }

    /// Changes the organization.
    pub fn org_mut<R>(&self, f: impl FnOnce(&mut MemOrg) -> R) -> R {
        f(&mut self.org.lock().expect("lock"))
    }

    fn attempt(&self, f: &TxnFn, base: &MemWork) -> (Result<TxnOutput, StoreError>, MemWork) {
        self.attempts.fetch_add(1, Ordering::SeqCst);
        let org = self.org.lock().expect("lock").clone();
        let mut txn = MemTxn {
            work: base.clone(),
            fail: &self.fail,
        };
        let out = f(&org, &mut txn);
        (out, txn.work)
    }
}

impl WorkStore for MemStore {
    fn transact(&self, f: Arc<TxnFn>) -> Result<TxnOutput, StoreError> {
        let mut work = self.work.lock().expect("lock");
        if self.rerun.swap(false, Ordering::SeqCst) {
            let _thrown_away = self.attempt(f.as_ref(), &work);
        }
        let (out, next) = self.attempt(f.as_ref(), &work);
        if out.is_ok() {
            *work = next;
        }
        out
    }
}

impl<T: WorkStore + ?Sized> WorkStore for Arc<T> {
    fn transact(&self, f: Arc<TxnFn>) -> Result<TxnOutput, StoreError> {
        (**self).transact(f)
    }
}

impl WorkReads for MemStore {
    fn get(&self, id: &str) -> Result<Option<StoredItem>, StoreError> {
        Ok(self.work.lock().expect("lock").items.get(id).cloned())
    }
    fn by_number(
        &self,
        scope: &NumberScope,
        number: u64,
    ) -> Result<Option<StoredItem>, StoreError> {
        let work = self.work.lock().expect("lock");
        Ok(work
            .items
            .values()
            .find(|i| {
                i.number == Some(number)
                    && NumberScope::of(i.kind, i.project_id.as_deref()).as_ref() == Some(scope)
            })
            .cloned())
    }
    fn by_short_id(&self, short: &str) -> Result<Vec<StoredItem>, StoreError> {
        let work = self.work.lock().expect("lock");
        Ok(work
            .items
            .values()
            .filter(|i| crate::ids::short_id(&i.id) == short)
            .cloned()
            .collect())
    }
    fn events(
        &self,
        target_id: &str,
        after_version: u64,
        limit: u32,
    ) -> Result<Vec<WorkEvent>, StoreError> {
        let work = self.work.lock().expect("lock");
        Ok(work
            .events
            .get(target_id)
            .map(|es| {
                es.iter()
                    .filter(|e| e.version > after_version)
                    .take(limit as usize)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default())
    }
    fn backlinks(&self, target_id: &str, limit: u32) -> Result<Vec<Link>, StoreError> {
        let work = self.work.lock().expect("lock");
        let deleted = |id: &str| work.items.get(id).is_some_and(|i| i.deleted);
        Ok(work
            .items
            .values()
            .flat_map(|i| {
                i.live_links()
                    .filter(
                        |l| matches!(&l.to, LinkTarget::Item { item_id } if item_id == target_id),
                    )
                    .map(|l| link_view(&i.id, l, &deleted))
                    .collect::<Vec<_>>()
            })
            .take(limit as usize)
            .collect())
    }
}

/// One transaction over a copy of the work data.
struct MemTxn<'a> {
    work: MemWork,
    fail: &'a Mutex<Option<FailPoint>>,
}

impl MemTxn<'_> {
    fn failing(&self, point: FailPoint) -> Result<(), StoreError> {
        let mut fail = self.fail.lock().expect("lock");
        if *fail == Some(point) {
            *fail = None;
            return Err(StoreError::new(format!("injected failure at {point:?}")));
        }
        Ok(())
    }
}

impl WorkTxn for MemTxn<'_> {
    fn get(&mut self, id: &str) -> Result<Option<StoredItem>, StoreError> {
        Ok(self.work.items.get(id).cloned())
    }
    fn item_of_step(&mut self, step_id: &str) -> Result<Option<StoredItem>, StoreError> {
        Ok(self
            .work
            .items
            .values()
            .find(|i| i.checklist.iter().any(|s| s.id == step_id))
            .cloned())
    }
    fn item_of_link(&mut self, link_id: &str) -> Result<Option<StoredItem>, StoreError> {
        Ok(self
            .work
            .items
            .values()
            .find(|i| i.links.iter().any(|l| l.id == link_id))
            .cloned())
    }
    fn id_in_use(&mut self, id: &str) -> Result<bool, StoreError> {
        Ok(self.work.items.values().any(|i| {
            i.id == id
                || i.checklist.iter().any(|s| s.id == id)
                || i.links.iter().any(|l| l.id == id)
                || i.comments.iter().any(|c| c.id == id)
        }))
    }
    fn events_since(
        &mut self,
        target_id: &str,
        after_version: u64,
    ) -> Result<Vec<WorkEvent>, StoreError> {
        Ok(self
            .work
            .events
            .get(target_id)
            .map(|es| {
                es.iter()
                    .filter(|e| e.version > after_version)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default())
    }
    fn change_result(&mut self, change_id: &str) -> Result<Option<StoredResult>, StoreError> {
        Ok(self.work.results.get(change_id).cloned())
    }
    fn allocate_number(&mut self, scope: &NumberScope) -> Result<u64, StoreError> {
        let next = self.work.numbers.entry(scope.key()).or_insert(0);
        *next += 1;
        Ok(*next)
    }
    fn put_item(&mut self, item: &StoredItem, events: &[WorkEvent]) -> Result<(), StoreError> {
        self.failing(FailPoint::PutItem)?;
        let stored = self.work.items.get(&item.id).map_or(0, |i| i.version);
        let contiguous = events
            .iter()
            .enumerate()
            .all(|(i, e)| e.version == stored + 1 + i as u64 && e.target_id == item.id);
        if events.is_empty() || !contiguous || item.version != stored + events.len() as u64 {
            return Err(StoreError::new(format!(
                "version gap writing {} (stored {stored})",
                item.id
            )));
        }
        self.work.items.insert(item.id.clone(), item.clone());
        self.work
            .events
            .entry(item.id.clone())
            .or_default()
            .extend(events.iter().cloned());
        Ok(())
    }
    fn put_change_result(
        &mut self,
        change_id: &str,
        result: &StoredResult,
    ) -> Result<(), StoreError> {
        self.failing(FailPoint::PutChangeResult)?;
        self.work
            .results
            .insert(change_id.to_owned(), result.clone());
        Ok(())
    }
    fn record_change(
        &mut self,
        org_position: Position,
        refs: &[EventRef],
        change_id: Option<&str>,
    ) -> Result<WorkPosition, StoreError> {
        self.failing(FailPoint::RecordChange)?;
        let position = WorkPosition {
            incarnation: 1,
            seq: self.work.feed.len() as u64 + 1,
        };
        self.work.feed.push(FeedRow {
            position,
            change_id: change_id.map(str::to_owned),
            org_position,
            refs: refs.to_vec(),
        });
        Ok(position)
    }
}

/// A clock tests set and move.
#[derive(Debug, Default)]
pub struct TestClock(AtomicU64);

impl TestClock {
    pub fn at(ms: u64) -> Arc<Self> {
        Arc::new(Self(AtomicU64::new(ms)))
    }
    pub fn set(&self, ms: u64) {
        self.0.store(ms, Ordering::SeqCst);
    }
    pub fn advance(&self, ms: u64) {
        self.0.fetch_add(ms, Ordering::SeqCst);
    }
}

impl Clock for TestClock {
    fn now_ms(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }
}
