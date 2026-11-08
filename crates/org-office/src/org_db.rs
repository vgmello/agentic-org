//! `org.db`: the organization's data and, on HQ, its event log. **All of its SQL lives here.**
//!
//! Each entity is stored as the contract's own JSON (`org_proto::org`) in a `body` column, beside
//! the few plain columns a query or a uniqueness rule needs. The contract is additive-only, so a
//! newer field simply travels in the body, and there is no mapping layer between the wire
//! type and the row to get wrong. The rules (`crate::rules`) read through [`OrgSql`] as an
//! [`OrgView`] and write through it as an [`OrgWrite`], inside the writer's savepoint, so a
//! command sees the effects of the one before it.
//!
//! The log (`events`) stores the changes of one command per entry, verbatim, at the next
//! position `(epoch 1, seq)`: no gaps. Its first entry is the organization's genesis: the owner.
//! An organization is *committed* once that entry is: `Office::discard` refuses a folder whose
//! `org.db` has one.

use rusqlite::{Connection, OptionalExtension};
use serde::de::DeserializeOwned;
use serde::Serialize;

use org_proto::org::{
    Area, Member, MemberKind, Membership, OrgChange, OrgSnapshot, Position, Project, ProjectTeam,
    Team,
};

use crate::error::OfficeError;
use crate::rules::{self, OrgView, OrgWrite};
use crate::store::Schema;

/// The epoch of every entry until promotion exists (phase 4).
const EPOCH: u64 = 1;

pub(crate) const SCHEMA: Schema = Schema {
    version: 1,
    create: |tx| {
        tx.execute_batch(
            "CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL) STRICT;
             CREATE TABLE events (
                 epoch INTEGER NOT NULL, seq INTEGER NOT NULL,
                 kind TEXT NOT NULL, command_id TEXT NOT NULL, at TEXT NOT NULL,
                 body TEXT NOT NULL,
                 PRIMARY KEY (epoch, seq)
             ) STRICT;
             CREATE TABLE commands (
                 command_id TEXT PRIMARY KEY, epoch INTEGER NOT NULL, seq INTEGER NOT NULL,
                 expires_at TEXT NOT NULL, result TEXT
             ) STRICT;
             CREATE TABLE teams (
                 id TEXT PRIMARY KEY, owner_team_id TEXT, name_key TEXT NOT NULL,
                 lead_member_id TEXT, body TEXT NOT NULL
             ) STRICT;
             CREATE UNIQUE INDEX teams_sibling ON teams (COALESCE(owner_team_id, ''), name_key);
             CREATE INDEX teams_owner ON teams (owner_team_id);
             CREATE INDEX teams_lead ON teams (lead_member_id);
             CREATE TABLE members (
                 id TEXT PRIMARY KEY, handle_key TEXT, reports_to_member_id TEXT,
                 body TEXT NOT NULL
             ) STRICT;
             CREATE UNIQUE INDEX members_handle ON members (handle_key);
             CREATE INDEX members_reports_to ON members (reports_to_member_id);
             CREATE TABLE memberships (
                 id TEXT PRIMARY KEY, team_id TEXT NOT NULL, member_id TEXT NOT NULL,
                 body TEXT NOT NULL,
                 UNIQUE (team_id, member_id)
             ) STRICT;
             CREATE INDEX memberships_member ON memberships (member_id);
             CREATE TABLE project_teams (
                 id TEXT PRIMARY KEY, project_id TEXT NOT NULL, team_id TEXT NOT NULL,
                 body TEXT NOT NULL,
                 UNIQUE (project_id, team_id)
             ) STRICT;
             CREATE INDEX project_teams_team ON project_teams (team_id);
             CREATE TABLE projects (id TEXT PRIMARY KEY, body TEXT NOT NULL) STRICT;
             CREATE TABLE areas (
                 id TEXT PRIMARY KEY, project_id TEXT NOT NULL, body TEXT NOT NULL
             ) STRICT;",
        )
    },
};

/// The rules' view of the organization and their way to write it, over the writer's connection
/// (or a reader's, for reads). A write is an upsert by id that NEVER replaces another row: a
/// clash on a sibling name, a handle or a (team, member) pair is a constraint error (a panic
/// here, so the request's savepoint rolls back), not a quiet deletion. The traits have no error
/// channel: a failing query panics with a
/// fixed text, which the writer reports as a storage failure of that one request and a reader
/// surfaces as an error.
pub(crate) struct OrgSql<'a>(pub(crate) &'a Connection);

fn sql<T>(result: rusqlite::Result<T>) -> T {
    result.expect("org.db: a query failed")
}

fn json<T: Serialize>(value: &T) -> String {
    serde_json::to_string(value).expect("org.db: an entity serializes")
}

fn entity<T: DeserializeOwned>(body: &str) -> T {
    serde_json::from_str(body).expect("org.db: a stored entity decodes")
}

impl OrgSql<'_> {
    fn one<T: DeserializeOwned>(&self, query: &str, params: impl rusqlite::Params) -> Option<T> {
        let body: Option<String> =
            sql(self.0.query_row(query, params, |row| row.get(0)).optional());
        body.map(|body| entity(&body))
    }

    fn many<T: DeserializeOwned>(&self, query: &str, params: impl rusqlite::Params) -> Vec<T> {
        let mut statement = sql(self.0.prepare_cached(query));
        let rows = sql(statement.query_map(params, |row| row.get::<_, String>(0)));
        rows.map(|body| entity(&sql(body))).collect()
    }

    fn exists(&self, query: &str, params: impl rusqlite::Params) -> bool {
        sql(self.0.query_row(query, params, |row| row.get::<_, bool>(0)))
    }
}

/// The key a member's handle is compared by, or `None` for the owner (whose display name is not
/// a handle).
fn handle_key(member: &Member) -> Option<String> {
    if member.owner {
        return None;
    }
    Some(match (&member.kind, &member.agent) {
        (MemberKind::Agent, Some(profile)) => profile.messaging_name.to_lowercase(),
        _ => rules::name_key(&member.name),
    })
}

impl OrgView for OrgSql<'_> {
    fn team(&self, id: &str) -> Option<Team> {
        self.one("SELECT body FROM teams WHERE id = ?1", [id])
    }
    fn team_named(&self, owner_team_id: Option<&str>, name_key: &str) -> Option<Team> {
        self.one(
            "SELECT body FROM teams WHERE name_key = ?1
             AND COALESCE(owner_team_id, '') = COALESCE(?2, '')",
            (name_key, owner_team_id),
        )
    }
    fn teams_owned_by(&self, team_id: &str) -> Vec<Team> {
        self.many(
            "SELECT body FROM teams WHERE owner_team_id = ?1 ORDER BY id",
            [team_id],
        )
    }
    fn teams_led_by(&self, member_id: &str) -> Vec<Team> {
        self.many(
            "SELECT body FROM teams WHERE lead_member_id = ?1 ORDER BY id",
            [member_id],
        )
    }
    fn member(&self, id: &str) -> Option<Member> {
        self.one("SELECT body FROM members WHERE id = ?1", [id])
    }
    fn member_with_handle(&self, handle_key: &str) -> Option<Member> {
        self.one(
            "SELECT body FROM members WHERE handle_key = ?1",
            [handle_key],
        )
    }
    fn reports_of(&self, member_id: &str) -> Vec<Member> {
        self.many(
            "SELECT body FROM members WHERE reports_to_member_id = ?1 ORDER BY id",
            [member_id],
        )
    }
    fn membership(&self, id: &str) -> Option<Membership> {
        self.one("SELECT body FROM memberships WHERE id = ?1", [id])
    }
    fn membership_of(&self, team_id: &str, member_id: &str) -> Option<Membership> {
        self.one(
            "SELECT body FROM memberships WHERE team_id = ?1 AND member_id = ?2",
            (team_id, member_id),
        )
    }
    fn memberships_of_team(&self, team_id: &str) -> Vec<Membership> {
        self.many(
            "SELECT body FROM memberships WHERE team_id = ?1 ORDER BY id",
            [team_id],
        )
    }
    fn memberships_of_member(&self, member_id: &str) -> Vec<Membership> {
        self.many(
            "SELECT body FROM memberships WHERE member_id = ?1 ORDER BY id",
            [member_id],
        )
    }
    fn project_exists(&self, project_id: &str) -> bool {
        self.exists(
            "SELECT EXISTS (SELECT 1 FROM projects WHERE id = ?1)",
            [project_id],
        )
    }
    fn project_team(&self, id: &str) -> Option<ProjectTeam> {
        self.one("SELECT body FROM project_teams WHERE id = ?1", [id])
    }
    fn project_team_of(&self, project_id: &str, team_id: &str) -> Option<ProjectTeam> {
        self.one(
            "SELECT body FROM project_teams WHERE project_id = ?1 AND team_id = ?2",
            (project_id, team_id),
        )
    }
    fn project_teams_of_team(&self, team_id: &str) -> Vec<ProjectTeam> {
        self.many(
            "SELECT body FROM project_teams WHERE team_id = ?1 ORDER BY id",
            [team_id],
        )
    }
}

impl OrgWrite for OrgSql<'_> {
    fn put_team(&mut self, team: &Team) {
        sql(self.0.execute(
            "INSERT INTO teams (id, owner_team_id, name_key, lead_member_id, body)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (id) DO UPDATE SET owner_team_id = excluded.owner_team_id,
                 name_key = excluded.name_key, lead_member_id = excluded.lead_member_id,
                 body = excluded.body",
            (
                &team.id,
                &team.owner_team_id,
                rules::name_key(&team.name),
                &team.lead_member_id,
                json(team),
            ),
        ));
    }
    fn delete_team(&mut self, id: &str) {
        sql(self.0.execute("DELETE FROM teams WHERE id = ?1", [id]));
    }
    fn put_member(&mut self, member: &Member) {
        sql(self.0.execute(
            "INSERT INTO members (id, handle_key, reports_to_member_id, body)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (id) DO UPDATE SET handle_key = excluded.handle_key,
                 reports_to_member_id = excluded.reports_to_member_id, body = excluded.body",
            (
                &member.id,
                handle_key(member),
                &member.reports_to_member_id,
                json(member),
            ),
        ));
    }
    fn delete_member(&mut self, id: &str) {
        sql(self.0.execute("DELETE FROM members WHERE id = ?1", [id]));
    }
    fn put_membership(&mut self, membership: &Membership) {
        sql(self.0.execute(
            "INSERT INTO memberships (id, team_id, member_id, body)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (id) DO UPDATE SET team_id = excluded.team_id,
                 member_id = excluded.member_id, body = excluded.body",
            (
                &membership.id,
                &membership.team_id,
                &membership.member_id,
                json(membership),
            ),
        ));
    }
    fn delete_membership(&mut self, id: &str) {
        sql(self
            .0
            .execute("DELETE FROM memberships WHERE id = ?1", [id]));
    }
    fn put_project_team(&mut self, project_team: &ProjectTeam) {
        sql(self.0.execute(
            "INSERT INTO project_teams (id, project_id, team_id, body)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (id) DO UPDATE SET project_id = excluded.project_id,
                 team_id = excluded.team_id, body = excluded.body",
            (
                &project_team.id,
                &project_team.project_id,
                &project_team.team_id,
                json(project_team),
            ),
        ));
    }
    fn delete_project_team(&mut self, id: &str) {
        sql(self
            .0
            .execute("DELETE FROM project_teams WHERE id = ?1", [id]));
    }
}

/// Applies `changes` in order. A change this build does not know is an error, so the caller's
/// savepoint rolls back whatever was written before it.
pub(crate) fn apply_changes(db: &Connection, changes: &[OrgChange]) -> Result<(), OfficeError> {
    let mut state = OrgSql(db);
    for change in changes {
        rules::apply(&mut state, change)
            .map_err(|_| OfficeError::Invalid("a change this build does not know"))?;
    }
    Ok(())
}

/// The position of the last log entry, or `None` for an empty log.
pub(crate) fn last_position(db: &Connection) -> Result<Option<Position>, OfficeError> {
    db.query_row(
        "SELECT epoch, seq FROM events ORDER BY epoch DESC, seq DESC LIMIT 1",
        (),
        |row| {
            // SQLite integers are signed; epochs and sequence numbers never get near the top.
            Ok(Position {
                epoch: row.get::<_, i64>(0)? as u64,
                seq: row.get::<_, i64>(1)? as u64,
            })
        },
    )
    .optional()
    .map_err(OfficeError::storage)
}

/// Appends one entry at the next position (no gaps) with `changes` verbatim.
pub(crate) fn append_event(
    db: &Connection,
    command_id: &str,
    at: &str,
    changes: &[OrgChange],
) -> Result<Position, OfficeError> {
    let next = match last_position(db)? {
        Some(last) => Position {
            epoch: last.epoch,
            seq: last.seq + 1,
        },
        None => Position {
            epoch: EPOCH,
            seq: 1,
        },
    };
    db.execute(
        "INSERT INTO events (epoch, seq, kind, command_id, at, body)
         VALUES (?1, ?2, 'org.changed', ?3, ?4, ?5)",
        (
            next.epoch as i64,
            next.seq as i64,
            command_id,
            at,
            json(&changes),
        ),
    )
    .map_err(OfficeError::storage)?;
    Ok(next)
}

/// Whether the log has an entry: the organization is committed.
pub(crate) fn is_committed(db: &Connection) -> Result<bool, OfficeError> {
    let has_log: bool = db
        .query_row(
            "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE name = 'events')",
            (),
            |row| row.get(0),
        )
        .map_err(OfficeError::storage)?;
    if !has_log {
        return Ok(false);
    }
    db.query_row("SELECT EXISTS (SELECT 1 FROM events)", (), |row| row.get(0))
        .map_err(OfficeError::storage)
}

/// What the organization is made of at creation.
pub(crate) struct Genesis {
    pub org_id: String,
    pub org_name: String,
    pub created_at: String,
    /// The owner's member id: made by the office, 26 letters and digits.
    pub owner_id: String,
}

/// Commits the organization: its identity rows and the first log entry, `(1, 1)`, whose only
/// change is the owner. One transaction (the caller's savepoint), so an organization is either
/// not there or whole.
pub(crate) fn genesis(db: &Connection, genesis: &Genesis) -> Result<Position, OfficeError> {
    for (key, value) in [
        ("organization_id", genesis.org_id.as_str()),
        ("organization_name", genesis.org_name.as_str()),
        ("created_at", genesis.created_at.as_str()),
    ] {
        db.execute(
            "INSERT INTO meta (key, value) VALUES (?1, ?2)",
            (key, value),
        )
        .map_err(OfficeError::storage)?;
    }
    let changes = rules::owner_bootstrap(&genesis.owner_id, "Owner", &genesis.created_at)
        .map_err(|_| OfficeError::Invalid("the owner could not be made"))?;
    apply_changes(db, &changes)?;
    append_event(db, "genesis", &genesis.created_at, &changes)
}

/// Everything the org chart needs, as of the last log entry, from one consistent read.
pub(crate) fn snapshot(db: &Connection) -> Result<OrgSnapshot, OfficeError> {
    // A deferred read transaction: every query below sees the same committed state.
    let tx = db.unchecked_transaction().map_err(OfficeError::storage)?;
    let position = last_position(&tx)?.ok_or(OfficeError::NotFound)?;
    let view = OrgSql(&tx);
    // Reads panic on a failing query (see `OrgSql`); a snapshot turns that into an error.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| OrgSnapshot {
        position,
        teams: view.many::<Team>("SELECT body FROM teams ORDER BY id", ()),
        members: view.many::<Member>("SELECT body FROM members ORDER BY id", ()),
        memberships: view.many::<Membership>("SELECT body FROM memberships ORDER BY id", ()),
        projects: view.many::<Project>("SELECT body FROM projects ORDER BY id", ()),
        project_teams: view.many::<ProjectTeam>("SELECT body FROM project_teams ORDER BY id", ()),
        areas: view.many::<Area>("SELECT body FROM areas ORDER BY id", ()),
    }));
    result.map_err(|_| OfficeError::storage(std::io::Error::other("a snapshot query failed")))
}

#[cfg(test)]
mod tests {
    use org_proto::org::{
        AgentProfile, AgentRuntime, AgentStatus, MemberAddPersonParams, MemberKind,
        MemberRemoveParams, MemberSetReportsToParams, MemberUpdatePersonParams,
        ProjectAddTeamParams, ProjectRemoveTeamParams, TeamAddMemberParams, TeamCreateParams,
        TeamDeleteParams, TeamMoveParams, TeamRemoveMemberParams, TeamRenameParams,
        TeamSetLeadParams,
    };
    use tempfile::TempDir;

    use super::*;
    use crate::key::Key;
    use crate::rules::memory::MemState;
    use crate::rules::{decide, OrgCommand};
    use crate::store::{Opening, Store};

    const NOW: &str = "2026-11-08T12:00:00.000Z";
    const OWNER: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";

    fn store() -> (TempDir, Store) {
        let dir = TempDir::new().unwrap();
        let store = Store::open(
            &dir.path().join("org.db"),
            &Key::from_bytes([5; 32]),
            Opening::New,
            &SCHEMA,
        )
        .unwrap();
        let genesis = Genesis {
            org_id: "ORG1".into(),
            org_name: "Acme".into(),
            created_at: NOW.into(),
            owner_id: OWNER.into(),
        };
        store
            .write(move |db| genesis_in(db, &genesis).map(|_| ((), None)))
            .unwrap();
        (dir, store)
    }

    fn genesis_in(db: &Connection, g: &Genesis) -> Result<Position, OfficeError> {
        genesis(db, g)
    }

    #[test]
    fn genesis_is_the_first_log_entry_and_holds_only_the_owner() {
        let (_dir, store) = store();
        let snapshot = store.read(snapshot).unwrap();
        assert_eq!(snapshot.position, Position { epoch: 1, seq: 1 });
        assert_eq!(snapshot.members.len(), 1);
        let owner = &snapshot.members[0];
        assert!(owner.owner);
        assert_eq!(
            (owner.id.as_str(), owner.name.as_str(), owner.version),
            (OWNER, "Owner", 1)
        );
        assert!(snapshot.teams.is_empty() && snapshot.memberships.is_empty());
        assert!(store.read(is_committed).unwrap());
    }

    #[test]
    fn a_log_entry_decodes_forever_and_positions_have_no_gaps() {
        let (_dir, store) = store();
        // A fixed entry as stored: the changes verbatim, in the contract's JSON.
        let body: String = store
            .read(|db| {
                db.query_row("SELECT body FROM events WHERE seq = 1", (), |r| r.get(0))
                    .map_err(OfficeError::storage)
            })
            .unwrap();
        let changes: Vec<OrgChange> = serde_json::from_str(&body).unwrap();
        assert!(matches!(&changes[..], [OrgChange::MemberUpserted { member }] if member.owner));
        // The golden bytes of a team change: pinned so the stored format cannot drift.
        let golden =
            r#"[{"type":"team_upserted","team":{"id":"t1","name":"Product","version":1}}]"#;
        let decoded: Vec<OrgChange> = serde_json::from_str(golden).unwrap();
        assert_eq!(json(&decoded), golden);

        let positions: Vec<Position> = (0..3)
            .map(|_| {
                store
                    .write({
                        let decoded = decoded.clone();
                        move |db| append_event(db, "c", NOW, &decoded).map(|p| (p, None))
                    })
                    .unwrap()
            })
            .collect();
        assert_eq!(
            positions
                .iter()
                .map(|p| (p.epoch, p.seq))
                .collect::<Vec<_>>(),
            [(1, 2), (1, 3), (1, 4)]
        );
    }

    fn id(n: u32) -> String {
        format!("{n:026}")
    }

    fn commands() -> Vec<OrgCommand> {
        let c = |n: u32| format!("cmd{n}");
        let team = |n: u32, team_id: &str, name: &str, owner: Option<&str>| {
            OrgCommand::TeamCreate(TeamCreateParams {
                command_id: c(n),
                team_id: team_id.into(),
                name: name.into(),
                owner_team_id: owner.map(Into::into),
            })
        };
        let person = |n: u32, member: &str, name: &str| {
            OrgCommand::MemberAddPerson(MemberAddPersonParams {
                command_id: c(n),
                member_id: member.into(),
                name: name.into(),
                contact: None,
            })
        };
        let join = |n: u32, m: &str, t: &str, who: &str| {
            OrgCommand::TeamAddMember(TeamAddMemberParams {
                command_id: c(n),
                membership_id: m.into(),
                team_id: t.into(),
                member_id: who.into(),
                title_in_team: None,
            })
        };
        vec![
            team(1, "t1", "Product", None),
            team(2, "t2", "Design", Some("t1")),
            team(3, "t3", "design", Some("t1")), // clashes with t2 ignoring case
            team(4, "t4", "Platform", None),
            team(41, "t5", "design", None), // the same name under no owner: fine
            team(42, "t6", "PRODUCT", None), // a top-level clash with t1
            person(5, &id(1), "Ann"),
            person(6, &id(2), "Bob"),
            person(7, &id(3), "ann"),    // a handle clash
            person(71, &id(4), "PIXEL"), // clashes with the agent's messaging name, any case
            OrgCommand::MemberUpdatePerson(MemberUpdatePersonParams {
                command_id: c(72),
                member_id: OWNER.into(),
                expected_version: 1,
                name: Some("Ann".into()), // the owner's display name is not a handle
                contact: None,
                clear_contact: false,
            }),
            join(8, "m1", "t2", &id(1)),
            join(9, "m2", "t2", &id(2)),
            join(10, "m1", "t4", &id(2)), // membership id taken
            OrgCommand::TeamSetLead(TeamSetLeadParams {
                command_id: c(11),
                team_id: "t2".into(),
                expected_version: 1,
                lead_member_id: Some(id(1)),
            }),
            OrgCommand::MemberSetReportsTo(MemberSetReportsToParams {
                command_id: c(12),
                member_id: id(2),
                expected_version: 1,
                reports_to_member_id: Some(id(1)),
            }),
            OrgCommand::MemberSetReportsTo(MemberSetReportsToParams {
                command_id: c(13),
                member_id: id(1),
                expected_version: 1,
                reports_to_member_id: Some(id(2)), // a loop
            }),
            OrgCommand::TeamMove(TeamMoveParams {
                command_id: c(14),
                team_id: "t1".into(),
                expected_version: 1,
                owner_team_id: Some("t2".into()), // a loop
            }),
            OrgCommand::TeamRename(TeamRenameParams {
                command_id: c(15),
                team_id: "t4".into(),
                expected_version: 1,
                name: "Platform team".into(),
            }),
            OrgCommand::ProjectAddTeam(ProjectAddTeamParams {
                command_id: c(60),
                project_team_id: "pt1".into(),
                project_id: "p1".into(),
                team_id: "t4".into(),
            }),
            OrgCommand::ProjectAddTeam(ProjectAddTeamParams {
                command_id: c(61),
                project_team_id: "pt2".into(),
                project_id: "p1".into(),
                team_id: "t2".into(),
            }),
            OrgCommand::ProjectAddTeam(ProjectAddTeamParams {
                command_id: c(62),
                project_team_id: "pt3".into(),
                project_id: "p1".into(),
                team_id: "t4".into(), // the pair is taken
            }),
            OrgCommand::ProjectRemoveTeam(ProjectRemoveTeamParams {
                command_id: c(63),
                project_team_id: "pt2".into(),
                expected_version: 1,
                areas_to_team_id: None,
            }),
            OrgCommand::TeamRemoveMember(TeamRemoveMemberParams {
                command_id: c(64),
                membership_id: "m2".into(),
                expected_version: 1,
            }),
            OrgCommand::TeamDelete(TeamDeleteParams {
                command_id: c(65),
                team_id: "t1".into(),
                expected_version: 1, // t1 owns t2: not empty
            }),
            OrgCommand::TeamDelete(TeamDeleteParams {
                command_id: c(66),
                team_id: "t4".into(), // takes its project link with it
                expected_version: 2,
            }),
            OrgCommand::MemberRemove(MemberRemoveParams {
                command_id: c(16),
                member_id: id(1),
                expected_version: 1,
            }),
        ]
    }

    #[test]
    fn the_sql_view_decides_exactly_like_the_in_memory_one() {
        let (_dir, store) = store();
        let mut mem = MemState::default();
        for change in crate::rules::owner_bootstrap(OWNER, "Owner", NOW).unwrap() {
            crate::rules::apply(&mut mem, &change).unwrap();
        }
        // An agent, placed straight into both (the agent commands come in a later slice).
        let agent = Member {
            id: id(9),
            kind: MemberKind::Agent,
            name: "Pixel the designer".into(),
            contact: None,
            owner: false,
            office_id: Some("office1".into()),
            agent: Some(AgentProfile {
                role: "designer".into(),
                title: "Designer".into(),
                focus: None,
                runtime: AgentRuntime::Codex,
                model: None,
                run_mode: None,
                schedule: None,
                home: None,
                messaging_name: "Pixel".into(),
                status: AgentStatus::Active,
                offboarded_at: None,
            }),
            reports_to_member_id: None,
            created_at: Some(NOW.into()),
            version: 1,
        };
        crate::rules::apply(
            &mut mem,
            &OrgChange::MemberUpserted {
                member: agent.clone(),
            },
        )
        .unwrap();
        store
            .write(move |db| {
                apply_changes(
                    db,
                    &[OrgChange::MemberUpserted {
                        member: agent.clone(),
                    }],
                )
                .map(|()| ((), None))
            })
            .unwrap();
        mem.projects.insert("p1".to_owned());
        store
            .write(|db| {
                db.execute(
                    r#"INSERT INTO projects (id, body)
                       VALUES ('p1', '{"id":"p1","name":"Project","version":1}')"#,
                    (),
                )
                .map_err(OfficeError::storage)?;
                Ok(((), None))
            })
            .unwrap();
        for command in commands() {
            let expected = decide(&mem, NOW, &command);
            let actual = store
                .write({
                    let command = command.clone();
                    move |db| {
                        let decided = decide(&OrgSql(db), NOW, &command);
                        if let Ok(changes) = &decided {
                            apply_changes(db, changes)?;
                        }
                        Ok((decided.map_err(|e| (e.code, e.message)), None))
                    }
                })
                .unwrap();
            match (&expected, &actual) {
                (Ok(a), Ok(b)) => {
                    assert_eq!(a, b, "{command:?}");
                    for change in a {
                        crate::rules::apply(&mut mem, change).unwrap();
                    }
                }
                (Err(a), Err((code, message))) => {
                    assert_eq!((a.code, &a.message), (*code, message), "{command:?}");
                }
                other => panic!("{command:?}: {other:?}"),
            }
        }
        // And the stored state is the in-memory one, entity for entity.
        let snapshot = store.read(snapshot).unwrap();
        assert_eq!(
            snapshot.teams,
            mem.teams.values().cloned().collect::<Vec<_>>()
        );
        assert_eq!(
            snapshot.members,
            mem.members.values().cloned().collect::<Vec<_>>()
        );
        assert_eq!(
            snapshot.memberships,
            mem.memberships.values().cloned().collect::<Vec<_>>()
        );
        assert_eq!(
            snapshot.project_teams,
            mem.project_teams.values().cloned().collect::<Vec<_>>()
        );
        assert!(!snapshot.teams.is_empty() && snapshot.members.len() >= 2);
        // The sequence really did add, remove and cascade-delete project links and memberships.
        assert!(snapshot.project_teams.is_empty());
        assert!(snapshot.memberships.iter().all(|m| m.id != "m2"));
    }

    fn put_fails(store: &Store, put: impl FnOnce(&mut OrgSql<'_>) + Send + 'static) -> bool {
        let mut put = Some(put);
        store
            .write(move |db| {
                let put = put.take().expect("runs once");
                put(&mut OrgSql(db));
                Ok(((), None))
            })
            .is_err()
    }

    fn team(id: &str, name: &str, owner: Option<&str>) -> Team {
        Team {
            id: id.into(),
            name: name.into(),
            owner_team_id: owner.map(Into::into),
            lead_member_id: None,
            version: 1,
        }
    }

    #[test]
    fn the_database_backs_up_the_sibling_name_and_handle_rules_and_never_deletes_to_do_it() {
        let (_dir, store) = store();
        store
            .write(|db| {
                let mut state = OrgSql(db);
                state.put_team(&team("a", "X", None));
                Ok(((), None))
            })
            .unwrap();
        // The same name (any case) among the top-level teams: a constraint error, and `a` stays.
        assert!(put_fails(&store, |s| s.put_team(&team("b", "x", None))));
        let after = store.read(snapshot).unwrap();
        assert_eq!(
            after
                .teams
                .iter()
                .map(|t| t.id.as_str())
                .collect::<Vec<_>>(),
            ["a"]
        );
        // Updating a team in place is fine.
        store
            .write(|db| {
                OrgSql(db).put_team(&team("a", "X", None));
                Ok(((), None))
            })
            .unwrap();
        // Two members with one handle: the second write fails and the first member stays.
        let person = |id: &str, name: &str| Member {
            id: id.into(),
            kind: MemberKind::Person,
            name: name.into(),
            contact: None,
            owner: false,
            office_id: None,
            agent: None,
            reports_to_member_id: None,
            created_at: None,
            version: 1,
        };
        store
            .write({
                let first = person("m1", "Ann");
                move |db| {
                    OrgSql(db).put_member(&first);
                    Ok(((), None))
                }
            })
            .unwrap();
        let second = person("m2", "ANN");
        assert!(put_fails(&store, move |s| s.put_member(&second)));
        let ids: Vec<String> = store
            .read(snapshot)
            .unwrap()
            .members
            .into_iter()
            .map(|m| m.id)
            .collect();
        assert!(
            ids.contains(&"m1".to_owned()) && !ids.contains(&"m2".to_owned()),
            "{ids:?}"
        );
        // A second membership of the same member in the same team.
        let membership = |id: &str| Membership {
            id: id.into(),
            team_id: "a".into(),
            member_id: "m1".into(),
            title_in_team: None,
            version: 1,
        };
        store
            .write(move |db| {
                OrgSql(db).put_membership(&membership("ms1"));
                Ok(((), None))
            })
            .unwrap();
        assert!(put_fails(&store, move |s| s.put_membership(&Membership {
            id: "ms2".into(),
            team_id: "a".into(),
            member_id: "m1".into(),
            title_in_team: None,
            version: 1,
        })));
        assert_eq!(store.read(snapshot).unwrap().memberships.len(), 1);
    }

    #[test]
    fn a_snapshot_of_an_empty_log_is_not_found() {
        let dir = TempDir::new().unwrap();
        let store = Store::open(
            &dir.path().join("org.db"),
            &Key::from_bytes([5; 32]),
            Opening::New,
            &SCHEMA,
        )
        .unwrap();
        assert!(matches!(store.read(snapshot), Err(OfficeError::NotFound)));
    }
}
