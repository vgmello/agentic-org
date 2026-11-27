//! A real HQ office on the SQLite engine in a temporary folder (never a user's organization
//! workspace), seeded through the office's own operations with an agent, a task, a private
//! note, a direct message, a task budget and recorded usage, then stopped.

use std::path::{Path, PathBuf};

use org_office::{Custody, DataRoots, Key, Office, TestOffice};
use org_proto::hiring::{HireProposalWire, HrHire, HrHireParams};
use org_proto::{Outcome, Request};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};

/// Parses `value` into an operation's parameters.
fn params<T: DeserializeOwned>(value: Value) -> T {
    serde_json::from_value(value).expect("valid test parameters")
}

/// Unwraps an office answer that is only checked to succeed.
fn ok<T, E: std::fmt::Debug, F: std::fmt::Debug>(result: Result<Result<T, E>, F>) -> T {
    result.expect("office storage").expect("office answer")
}

/// Unwraps an office answer into JSON for comparison.
fn answer<T: serde::Serialize, E: std::fmt::Debug, F: std::fmt::Debug>(
    result: Result<Result<T, E>, F>,
) -> Value {
    serde_json::to_value(result.expect("office storage").expect("office answer")).unwrap()
}

pub struct Org {
    pub _tmp: tempfile::TempDir,
    pub root: PathBuf,
    pub office_home: PathBuf,
    pub folder: PathBuf,
    pub roots: DataRoots,
    pub custody: Custody,
    pub key: Key,
    pub org_id: String,
    pub office_id: String,
    pub owner: String,
    pub agent: String,
    pub task_id: String,
    pub note_id: String,
    pub conversation: String,
}

impl Org {
    /// A new organization, seeded and stopped.
    pub fn seeded() -> Org {
        let tmp = tempfile::tempdir().expect("a temporary directory");
        let root = tmp.path().canonicalize().expect("a real path");
        let (office, roots, custody, key) = Office::for_test_with_real_ca_in(
            &root,
            TestOffice::sqlite(org_office_sqlite::sqlite_backend()),
        );
        let owner = office.owner_member_id().to_owned();
        let team = office.org_snapshot().unwrap().teams[0].id.clone();
        let agent = hire(&office, &owner, &team);
        let task_id = office.next_uuid_v7();
        ok(office.task_create(
            &owner,
            params(json!({
                "command_id": office.next_uuid_v7(), "task_id": task_id, "team_id": team,
                "assignee_id": agent, "title": "Rotate the signing keys",
                "body_markdown": "Carefully.", "labels": ["security"],
            })),
        ));
        let note_id = office.next_uuid_v7();
        answer(office.task_note_add(
            &agent,
            params(json!({
                "command_id": office.next_uuid_v7(), "task": task_id, "note_id": note_id,
                "body_markdown": "private plan: rotate the HSM first",
            })),
        ));
        let dm = answer(office.dm_open(&owner, params(json!({ "participants": [agent] }))));
        let conversation = dm["conversation"]["id"].as_str().unwrap().to_owned();
        ok(office.chat_send(
            &owner,
            params(json!({
                "id": office.next_uuid_v7(), "to": conversation, "kind": "msg",
                "body": "please start with the staging keys",
            })),
        ));
        answer(office.budget_set(
            &owner,
            params(json!({
                "operation_id": office.next_uuid_v7(), "scope_kind": "task", "task_id": task_id,
                "period_kind": "lifetime", "unit": "micro_usd", "limit_amount": 25000,
                "reason": "initial cap",
            })),
        ));
        answer(office.cost_price_set(
            &owner,
            params(json!({
                "provider": "anthropic", "model": "claude-sonnet-4",
                "effective_from": "2026-03-01T00:00:00.000Z",
                "input_micro_usd_per_million": 1000000, "output_micro_usd_per_million": 2000000,
                "cache_read_micro_usd_per_million": 500000,
                "cache_write_micro_usd_per_million": 1250000,
            })),
        ));
        answer(office.cost_usage_record(
            &owner,
            params(json!({
                "producer_id": "prod-1", "source_event_id": "evt-1", "run_id": "run-1",
                "actor_kind": "agent", "employee_id": agent, "runtime": "claude_code",
                "provider": "anthropic", "model": "claude-sonnet-4",
                "occurred_at": "2026-03-08T10:00:00.000Z", "input_tokens": 10000,
                "output_tokens": 5000, "cache_read_tokens": 0, "cache_write_tokens": 0,
                "reasoning_tokens": 0, "is_subscription": false, "scope_kind": "task",
                "task_id": task_id, "task_type_snapshot": "task",
                "attribution_basis": "current_task",
            })),
        ));
        let org_id = office.info().org_id.clone();
        let office_id = office.info().office_id.clone();
        let folder = office.folder().to_path_buf();
        office.close().expect("an orderly stop");
        Org {
            office_home: root.join("home"),
            root,
            _tmp: tmp,
            folder,
            roots,
            custody,
            key,
            org_id,
            office_id,
            owner,
            agent,
            task_id,
            note_id,
            conversation,
        }
    }

    /// Opens the stopped office again, as the host would.
    pub fn open(&self) -> Office {
        Office::open(
            &self.folder,
            Some(self.key.clone()),
            self.custody.identity.instance_id.as_str(),
            self.roots.clone(),
            self.custody.clone(),
            org_office_sqlite::sqlite_backend(),
        )
        .expect("the office opens")
    }

    /// Everything the seeded data answers, read through the office's own queries.
    pub fn queries(&self, office: &Office) -> Value {
        json!({
            "note": answer(office.task_note_get(&self.agent, params(json!({ "note_id": self.note_id })))),
            "messages": answer(office.chat_messages(
                &self.owner,
                params(json!({ "conversation": self.conversation })),
            ))["messages"].clone(),
            "budgets": answer(office.budget_list(&self.owner, params(json!({})))),
            "costs": answer(office.cost_query(&self.owner, params(json!({ "task_id": self.task_id })))),
            "tasks": answer(office.task_list(params(json!({})))),
            "people": serde_json::to_value(office.org_snapshot().unwrap().members.len()).unwrap(),
        })
    }

    pub fn durable(&self) -> &Path {
        &self.roots.durable
    }

    pub fn runtime(&self) -> &Path {
        &self.roots.runtime
    }
}

fn hire(office: &Office, owner: &str, team: &str) -> String {
    let employee_id = office.next_uuid_v7();
    let request = Request::new::<HrHire>(
        1,
        &HrHireParams {
            command_id: office.next_uuid_v7(),
            request_id: office.next_uuid_v7(),
            employee_id: employee_id.clone(),
            home_team_id: team.into(),
            role_id: None,
            proposal: HireProposalWire {
                name: "Alpha Agent".into(),
                handle: "alpha".into(),
                title_in_team: Some("Engineer".into()),
                ..Default::default()
            },
            reason: "seed a test agent".into(),
        },
    )
    .expect("an hr.hire request");
    let response = office
        .dispatch_hiring_rpc(Some(owner), &request)
        .expect("hr.hire handled");
    assert!(
        matches!(response.outcome, Outcome::Result(_)),
        "hr.hire failed: {:?}",
        response.outcome
    );
    employee_id
}

/// Every file below `root` with its bytes, by relative path.
pub fn tree(root: &Path) -> std::collections::BTreeMap<String, Vec<u8>> {
    let mut out = std::collections::BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            let meta = std::fs::symlink_metadata(&path).unwrap();
            if meta.is_dir() {
                stack.push(path);
            } else {
                let relative = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .to_owned();
                out.insert(relative, std::fs::read(&path).unwrap());
            }
        }
    }
    out
}

/// A copy of the folder `from` at `to` (regular files and folders only).
pub fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for (relative, bytes) in tree(from) {
        let target = to.join(&relative);
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(target, bytes).unwrap();
    }
}
