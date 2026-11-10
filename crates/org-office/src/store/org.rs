//! `org.db`: the organization's data and, on HQ, its event log. A submodule of the store, so
//! "all SQL and every rusqlite type stay in one private module" (ADR-0002 §5) still holds.
//!
//! **Persisted shapes change only with `SCHEMA.version`.** An entity's body is the contract's own
//! JSON, and the contract's types drop fields and enum values they do not know when they decode.
//! So an older build that rewrote a newer body would lose data silently. Any contract change that
//! adds a persisted field or enum value therefore bumps `SCHEMA.version`, which makes older builds
//! refuse the file (`Store::open`), instead of relying on unknown fields travelling along.
//!
//! Each entity is stored as the contract's own JSON (`org_proto::org`) in a `body` column, beside
//! the few plain columns a query or a uniqueness rule needs, so there is no mapping layer between
//! the wire type and the row to get wrong. The rules (`crate::rules`) read through [`OrgSql`] as an
//! [`OrgView`] and write through [`OrgWriteSql`] as an [`OrgWrite`], inside the writer's savepoint, so a
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
    Area, CommandAccepted, CommandStatus, Member, MemberKind, Membership, OrgChange,
    OrgChangedData, OrgSnapshot, Position, Project, ProjectTeam, Team,
};
use org_proto::{ApiError, ErrorCode};

use super::Schema;
use crate::error::OfficeError;
use crate::rules::{self, OrgCommand, OrgView, OrgWrite};
use crate::{time, ulid};

mod replication;
pub(crate) use replication::transact_replication;

/// The epoch of every entry until promotion exists (phase 4).
const EPOCH: u64 = 1;

pub(crate) const SCHEMA: Schema = Schema {
    version: 2,
    upgrade: |tx, from| {
        if from < 2 {
            replication::create_tables(tx)?;
        }
        Ok(())
    },
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
        )?;
        replication::create_tables(tx)
    },
};

/// The rules' read view over the current transaction. `OrgView` has no error channel, so a
/// failing query panics and is reported as a storage failure at the transaction boundary.
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
    fn office_exists(&self, office_id: &str) -> bool {
        // HQ is the one office there is (its id is in `meta`); offices that enrol later get a
        // table of their own.
        self.exists(
            "SELECT EXISTS (SELECT 1 FROM meta WHERE key = 'office_id' AND value = ?1)",
            [office_id],
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

/// The domain's mechanical apply writes through `OrgWrite`, which has no error return.
/// Retain the SQLite result so the caller can fail the transaction with the original cause.
struct OrgWriteSql<'a> {
    db: &'a Connection,
    error: Option<rusqlite::Error>,
}

impl<'a> OrgWriteSql<'a> {
    fn new(db: &'a Connection) -> Self {
        Self { db, error: None }
    }

    fn write_result(&mut self, result: rusqlite::Result<usize>) {
        if self.error.is_none() {
            self.error = result.err();
        }
    }

    fn check_write(&mut self) -> Result<(), OfficeError> {
        match self.error.take() {
            Some(error) => Err(OfficeError::storage(error)),
            None => Ok(()),
        }
    }
}

// Upsert by id. A conflicting name, handle or membership returns the constraint error
// and lets the writer roll back, preserving the other row.
impl OrgWrite for OrgWriteSql<'_> {
    fn put_team(&mut self, team: &Team) {
        self.write_result(self.db.execute(
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
        self.write_result(self.db.execute("DELETE FROM teams WHERE id = ?1", [id]));
    }
    fn put_member(&mut self, member: &Member) {
        self.write_result(self.db.execute(
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
        self.write_result(self.db.execute("DELETE FROM members WHERE id = ?1", [id]));
    }
    fn put_membership(&mut self, membership: &Membership) {
        self.write_result(self.db.execute(
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
        self.write_result(
            self.db
                .execute("DELETE FROM memberships WHERE id = ?1", [id]),
        );
    }
    fn put_project_team(&mut self, project_team: &ProjectTeam) {
        self.write_result(self.db.execute(
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
        self.write_result(
            self.db
                .execute("DELETE FROM project_teams WHERE id = ?1", [id]),
        );
    }
}

/// Applies `changes` in order. A change this build does not know is an error, so the caller's
/// savepoint rolls back whatever was written before it.
pub(crate) fn apply_changes(db: &Connection, changes: &[OrgChange]) -> Result<(), OfficeError> {
    let mut state = OrgWriteSql::new(db);
    for change in changes {
        rules::apply(&mut state, change)
            .map_err(|_| OfficeError::Invalid("a change this build does not know"))?;
        state.check_write()?;
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

const DAY_MS: u64 = 86_400_000;
/// A command whose id is dated further back than this is `expired` (offices design, dedup window).
const MAX_AGE_MS: u64 = 29 * DAY_MS;
/// A command whose id is dated further ahead of HQ's clock than this is `clock_skew`.
const MAX_AHEAD_MS: u64 = 5 * 60_000;
/// How long an answer is kept: from the id's own time, so no clock can reopen the window.
const KEEP_MS: u64 = 30 * DAY_MS;
/// `meta` row: the highest clock reading HQ has used, so a clock that steps back cannot reopen
/// the window.
const CLOCK_HIGH_KEY: &str = "clock_high_ms";

/// What `commands.result` holds: a fingerprint of the command, and the refusal if HQ refused it
/// (a committed command's answer is its `(epoch, seq)` columns).
#[derive(Serialize, serde::Deserialize)]
struct Answer {
    params_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    refusal: Option<ApiError>,
}

fn fingerprint(command: &OrgCommand) -> String {
    use sha2::{Digest, Sha256};
    let (method, params) = command.method_and_params();
    let mut hasher = Sha256::new();
    hasher.update(method.as_bytes());
    hasher.update([0]);
    hasher.update(params.as_bytes());
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Runs one command at HQ (the offices design's step 3), in the caller's savepoint.
///
/// 1. **Lookup first.** A `command_id` HQ already answered gets that answer without deciding
///    again: the commit's position, or the same refusal (**rejections are remembered too**; to
///    retry a refused command the client uses a new id). The same id with other params is
///    `invalid_request`.
/// 2. The id must be a ULID (`invalid_request`), dated no more than 29 days back (`expired`) and
///    no more than 5 minutes ahead (`clock_skew`) of HQ's clock, which never goes back
///    (`clock_ms` is the clock's reading; the highest one used is kept). These are not stored.
/// 3. The rules decide. A refusal is stored under the id (position `(0, 0)`: log positions start
///    at `(1, 1)`); a decision is applied, logged and stored.
///
/// An answer is kept until its id's time + 30 days (`expires_at`); nothing purges them yet.
///
/// The fingerprint is a hash of the method and the params' JSON as the contract's types write
/// it, so a later contract change to a params type's serialization would change the hash of
/// stored answers; such a change must come with a migration of `commands`.
///
/// Every refusal the rules give is stored. The rules give only domain refusals (never a storage
/// or availability code, which `org-work` leaves unstored); a rule that ever does must be
/// excluded here.
pub(crate) fn submit(
    db: &Connection,
    clock_ms: u64,
    command: &OrgCommand,
) -> Result<Result<CommandAccepted, ApiError>, OfficeError> {
    let command_id = command.command_id();
    let params_hash = fingerprint(command);
    let seen = db
        .query_row(
            "SELECT epoch, seq, result FROM commands WHERE command_id = ?1",
            [command_id],
            |row| {
                Ok((
                    Position {
                        epoch: row.get::<_, i64>(0)? as u64,
                        seq: row.get::<_, i64>(1)? as u64,
                    },
                    row.get::<_, Option<String>>(2)?,
                ))
            },
        )
        .optional()
        .map_err(OfficeError::storage)?;
    if let Some((position, answer)) = seen {
        let answer: Answer = answer
            .and_then(|text| serde_json::from_str(&text).ok())
            .ok_or(OfficeError::Invalid(
                "a stored command answer is not readable",
            ))?;
        return Ok(if answer.params_hash != params_hash {
            Err(ApiError::new(
                ErrorCode::InvalidRequest,
                "command id reused with different params",
            ))
        } else if let Some(refusal) = answer.refusal {
            Err(refusal)
        } else {
            Ok(committed(position))
        });
    }

    let Some(made_ms) = ulid::time_ms(command_id) else {
        return Ok(Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "`command_id` must be a ULID",
        )));
    };
    // Absent until the first command; an unreadable mark is an error, not "no mark".
    let high = match db
        .query_row(
            "SELECT value FROM meta WHERE key = ?1",
            [CLOCK_HIGH_KEY],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(OfficeError::storage)?
    {
        None => 0,
        Some(value) => value
            .parse::<u64>()
            .map_err(|_| OfficeError::Invalid("the clock high-water mark is not readable"))?,
    };
    let now_ms = clock_ms.max(high);
    if now_ms > high {
        db.execute(
            "INSERT INTO meta (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            (CLOCK_HIGH_KEY, now_ms.to_string()),
        )
        .map_err(OfficeError::storage)?;
    }
    if now_ms.saturating_sub(made_ms) > MAX_AGE_MS {
        return Ok(Err(ApiError::new(
            ErrorCode::Expired,
            "the command id is too old",
        )));
    }
    if made_ms > now_ms.saturating_add(MAX_AHEAD_MS) {
        return Ok(Err(ApiError::new(
            ErrorCode::ClockSkew,
            "the command id is dated ahead of HQ's clock",
        )));
    }

    let remember = |position: Position, refusal: Option<ApiError>| {
        db.execute(
            "INSERT INTO commands (command_id, epoch, seq, expires_at, result)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            (
                command_id,
                position.epoch as i64,
                position.seq as i64,
                time::format_epoch_ms(made_ms.saturating_add(KEEP_MS)),
                json(&Answer {
                    params_hash: params_hash.clone(),
                    refusal,
                }),
            ),
        )
        .map_err(OfficeError::storage)
    };
    let now = time::format_epoch_ms(now_ms);
    let changes = match rules::decide(&OrgSql(db), &now, command) {
        Ok(changes) => changes,
        Err(refusal) => {
            remember(Position { epoch: 0, seq: 0 }, Some(refusal.clone()))?;
            return Ok(Err(refusal));
        }
    };
    apply_changes(db, &changes)?;
    let position = append_event(db, command_id, &now, &changes)?;
    remember(position, None)?;
    Ok(Ok(committed(position)))
}

fn committed(position: Position) -> CommandAccepted {
    CommandAccepted {
        status: CommandStatus::Committed,
        position: Some(position),
    }
}

/// Whether a subscriber that holds the organization as of `after` can be sent the changes
/// that follow: `after` is a position this log has (same epoch, at least the genesis, no later
/// than the head). Nothing is trimmed yet, so every earlier position of the epoch qualifies.
pub(crate) fn can_resume_from(db: &Connection, after: Position) -> Result<bool, OfficeError> {
    Ok(match last_position(db)? {
        Some(head) => after.epoch == head.epoch && (1..=head.seq).contains(&after.seq),
        None => false,
    })
}

/// Up to `limit` committed commands after `after`, oldest first, as the `org.changed` events'
/// data. The log stores each command's changes verbatim, so this is a read, not a rebuild.
pub(crate) fn changes_after(
    db: &Connection,
    after: Position,
    limit: usize,
) -> Result<Vec<OrgChangedData>, OfficeError> {
    let mut statement = db
        .prepare(
            "SELECT epoch, seq, command_id, body FROM events
             WHERE kind = 'org.changed' AND (epoch, seq) > (?1, ?2)
             ORDER BY epoch, seq LIMIT ?3",
        )
        .map_err(OfficeError::storage)?;
    let rows = statement
        .query_map(
            (after.epoch as i64, after.seq as i64, limit as i64),
            |row| {
                Ok((
                    Position {
                        epoch: row.get::<_, i64>(0)? as u64,
                        seq: row.get::<_, i64>(1)? as u64,
                    },
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )
        .map_err(OfficeError::storage)?;
    let mut out = Vec::new();
    for row in rows {
        let (position, command_id, body) = row.map_err(OfficeError::storage)?;
        let changes = serde_json::from_str(&body)
            .map_err(|_| OfficeError::Invalid("a logged entry is not readable"))?;
        out.push(OrgChangedData {
            position,
            command_id,
            changes,
        });
    }
    Ok(out)
}

/// The position of the last log entry; `NotFound` for an empty log (an unfinished create).
pub(crate) fn head(db: &Connection) -> Result<Position, OfficeError> {
    last_position(db)?.ok_or(OfficeError::NotFound)
}

/// Test seam: the organization as `members` (and nothing else) applied up to `position`.
#[cfg(test)]
pub(crate) fn seed_for_test(
    db: &Connection,
    members: &[Member],
    position: Position,
) -> Result<(), OfficeError> {
    db.execute_batch(
        "DELETE FROM memberships; DELETE FROM teams; DELETE FROM members; DELETE FROM events;",
    )
    .map_err(OfficeError::storage)?;
    let mut state = OrgWriteSql::new(db);
    for member in members {
        state.put_member(member);
        state.check_write()?;
    }
    db.execute(
        "INSERT INTO events (epoch, seq, kind, command_id, at, body)
         VALUES (?1, ?2, 'org.changed', 'seed', '2026-11-08T12:00:00.000Z', '[]')",
        (position.epoch as i64, position.seq as i64),
    )
    .map_err(OfficeError::storage)?;
    Ok(())
}

/// Whether the organization is committed: the log has an entry, or (an `org.db` from before the
/// log existed) the identity row `organization_id` is there. Anything less is an unfinished
/// create, which `Office::discard` may remove.
pub(crate) fn is_committed(db: &Connection) -> Result<bool, OfficeError> {
    let exists = |table: &str| -> Result<bool, OfficeError> {
        db.query_row(
            "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE name = ?1)",
            [table],
            |row| row.get(0),
        )
        .map_err(OfficeError::storage)
    };
    if exists("events")?
        && db
            .query_row("SELECT EXISTS (SELECT 1 FROM events)", (), |row| row.get(0))
            .map_err(OfficeError::storage)?
    {
        return Ok(true);
    }
    if exists("meta")? {
        return db
            .query_row(
                "SELECT EXISTS (SELECT 1 FROM meta WHERE key = 'organization_id')",
                (),
                |row| row.get(0),
            )
            .map_err(OfficeError::storage);
    }
    Ok(false)
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
        AgentProfile, AgentRuntime, AgentStatus, MemberAddAgentParams, MemberAddPersonParams,
        MemberKind, MemberOffboardParams, MemberRemoveParams, MemberSetAgentStatusParams,
        MemberSetReportsToParams, MemberSetRunModeParams, MemberUpdateAgentParams,
        MemberUpdatePersonParams, ProjectAddTeamParams, ProjectRemoveTeamParams, RunMode, Schedule,
        TeamAddMemberParams, TeamCreateParams, TeamDeleteParams, TeamMoveParams,
        TeamRemoveMemberParams, TeamRenameParams, TeamSetLeadParams,
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
        let hire = |n: u32, member: u32, name: &str, office: &str, handle: &str| {
            OrgCommand::MemberAddAgent(MemberAddAgentParams {
                command_id: c(n),
                member_id: id(member),
                name: name.into(),
                office_id: office.into(),
                profile: AgentProfile {
                    role: "engineer".into(),
                    title: "Engineer".into(),
                    focus: None,
                    runtime: AgentRuntime::ClaudeCode,
                    model: None,
                    run_mode: None,
                    schedule: None,
                    home: None,
                    messaging_name: handle.into(),
                    status: AgentStatus::Active,
                    offboarded_at: None,
                },
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
            person(73, &id(5), "old"),   // an offboarded agent's handle stays reserved
            OrgCommand::MemberUpdatePerson(MemberUpdatePersonParams {
                command_id: c(74),
                member_id: id(2),
                expected_version: 1,
                name: Some("OLD".into()), // ...also against a rename
                contact: None,
                clear_contact: false,
            }),
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
            // ---- agents: hire, edit, pause, schedule, offboard (with relations), refusals
            hire(90, 20, "Robo", "office1", "Robo.bot"),
            hire(91, 21, "Dup", "office1", "ROBO.bot"), // the handle, any case: already_exists
            hire(92, 22, "Lost", "nowhere", "lost"),    // no such office: not_found
            hire(93, 23, "Pixel2", "office1", "pixel"), // an agent's handle: already_exists
            hire(94, 24, "Old2", "office1", "old"), // an offboarded agent's handle: already_exists
            OrgCommand::MemberUpdateAgent(MemberUpdateAgentParams {
                command_id: c(95),
                member_id: id(20),
                expected_version: 1,
                name: Some("Robo Two".into()),
                role: Some("tester".into()),
                title: None,
                focus: Some("qa".into()),
                clear_focus: false,
                runtime: Some(AgentRuntime::Antigravity),
                model: None,
                clear_model: false,
                home: Some("/agents/robo".into()),
                clear_home: false,
                messaging_name: Some("ROBO.bot".into()),
            }),
            OrgCommand::MemberUpdateAgent(MemberUpdateAgentParams {
                command_id: c(96),
                member_id: id(20),
                expected_version: 1, // stale
                name: None,
                role: Some("x".into()),
                title: None,
                focus: None,
                clear_focus: false,
                runtime: None,
                model: None,
                clear_model: false,
                home: None,
                clear_home: false,
                messaging_name: None,
            }),
            OrgCommand::MemberSetAgentStatus(MemberSetAgentStatusParams {
                command_id: c(97),
                member_id: id(20),
                expected_version: 2,
                status: AgentStatus::Paused,
            }),
            OrgCommand::MemberSetRunMode(MemberSetRunModeParams {
                command_id: c(98),
                member_id: id(20),
                expected_version: 3,
                run_mode: Some(RunMode::Scheduled),
                schedule: Some(Schedule {
                    spec: "weekdays at 9".into(),
                    time_zone: "Europe/Lisbon".into(),
                }),
            }),
            OrgCommand::TeamCreate(TeamCreateParams {
                command_id: c(99),
                team_id: "ag-team".into(),
                name: "Agents".into(),
                owner_team_id: None,
            }),
            OrgCommand::TeamAddMember(TeamAddMemberParams {
                command_id: c(100),
                membership_id: "m-ag".into(),
                team_id: "ag-team".into(),
                member_id: id(20),
                title_in_team: None,
            }),
            OrgCommand::TeamSetLead(TeamSetLeadParams {
                command_id: c(101),
                team_id: "ag-team".into(),
                expected_version: 1,
                lead_member_id: Some(id(20)),
            }),
            OrgCommand::MemberOffboard(MemberOffboardParams {
                command_id: c(102),
                member_id: id(20),
                expected_version: 4,
            }),
            OrgCommand::MemberOffboard(MemberOffboardParams {
                command_id: c(103),
                member_id: id(20), // already offboarded: invalid_request
                expected_version: 5,
            }),
            hire(104, 25, "Again", "office1", "robo.bot"), // reserved for good: already_exists
        ]
    }

    #[test]
    fn the_sql_view_decides_exactly_like_the_in_memory_one() {
        let (_dir, store) = store();
        let mut mem = MemState::default();
        for change in crate::rules::owner_bootstrap(OWNER, "Owner", NOW).unwrap() {
            crate::rules::apply(&mut mem, &change).unwrap();
        }
        // Two agents, placed straight into both (a live one and an offboarded one).
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
        let mut old = agent.clone();
        old.id = id(8);
        old.name = "Old the retired".into();
        if let Some(profile) = old.agent.as_mut() {
            profile.messaging_name = "Old".into();
            profile.status = AgentStatus::Offboarded;
            profile.offboarded_at = Some(NOW.into());
        }
        for member in [&agent, &old] {
            crate::rules::apply(
                &mut mem,
                &OrgChange::MemberUpserted {
                    member: member.clone(),
                },
            )
            .unwrap();
        }
        store
            .write(move |db| {
                apply_changes(
                    db,
                    &[
                        OrgChange::MemberUpserted {
                            member: agent.clone(),
                        },
                        OrgChange::MemberUpserted {
                            member: old.clone(),
                        },
                    ],
                )
                .map(|()| ((), None))
            })
            .unwrap();
        mem.projects.insert("p1".to_owned());
        mem.offices.insert("office1".to_owned());
        store.run_sql_for_test("INSERT INTO meta (key, value) VALUES ('office_id', 'office1')");
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
        // And the agent commands really ran: hired, edited, paused, scheduled, offboarded with
        // its membership ended and the team it led left without a lead.
        let robo = snapshot
            .members
            .iter()
            .find(|m| m.id == id(20))
            .expect("the agent was hired");
        let profile = robo.agent.as_ref().unwrap();
        assert_eq!(robo.version, 5);
        assert_eq!(robo.name, "Robo Two");
        assert_eq!(profile.status, AgentStatus::Offboarded);
        assert_eq!(profile.run_mode, Some(RunMode::Scheduled));
        assert_eq!(profile.messaging_name, "ROBO.bot");
        assert!(profile.offboarded_at.is_some());
        assert!(snapshot.memberships.iter().all(|m| m.id != "m-ag"));
        let agents_team = snapshot.teams.iter().find(|t| t.id == "ag-team").unwrap();
        assert_eq!(agents_team.lead_member_id, None);
        assert!(snapshot
            .members
            .iter()
            .all(|m| m.id != id(21) && m.id != id(25)));
    }

    fn put_for_test(
        store: &Store,
        put: impl Fn(&mut OrgWriteSql<'_>) + Send + 'static,
    ) -> Result<(), OfficeError> {
        store.write(move |db| {
            let mut state = OrgWriteSql::new(db);
            put(&mut state);
            state.check_write()?;
            Ok(((), None))
        })
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
    fn organization_apply_preserves_the_sqlite_error_and_rolls_back_earlier_changes() {
        use std::error::Error;

        let (_dir, store) = store();
        let error = store
            .write(|db| {
                apply_changes(
                    db,
                    &[
                        OrgChange::TeamUpserted {
                            team: team("one", "Same name", None),
                        },
                        OrgChange::TeamUpserted {
                            team: team("two", "SAME NAME", None),
                        },
                    ],
                )?;
                Ok(((), None))
            })
            .unwrap_err();
        assert!(matches!(
            error.source().and_then(|source| source.downcast_ref()),
            Some(rusqlite::Error::SqliteFailure(failure, _))
                if failure.code == rusqlite::ErrorCode::ConstraintViolation
        ));
        assert!(store.read(snapshot).unwrap().teams.is_empty());
    }

    #[test]
    fn the_database_backs_up_the_sibling_name_and_handle_rules_and_never_deletes_to_do_it() {
        let (_dir, store) = store();
        put_for_test(&store, |s| s.put_team(&team("a", "X", None))).unwrap();
        // The same name (any case) among the top-level teams: a constraint error, and `a` stays.
        assert!(put_for_test(&store, |s| s.put_team(&team("b", "x", None))).is_err());
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
        put_for_test(&store, |s| s.put_team(&team("a", "X", None))).unwrap();
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
        let first = person("m1", "Ann");
        put_for_test(&store, move |s| s.put_member(&first)).unwrap();
        let second = person("m2", "ANN");
        assert!(put_for_test(&store, move |s| s.put_member(&second)).is_err());
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
        put_for_test(&store, move |s| s.put_membership(&membership("ms1"))).unwrap();
        assert!(put_for_test(&store, move |s| s.put_membership(&membership("ms2"))).is_err());
        assert_eq!(store.read(snapshot).unwrap().memberships.len(), 1);
    }

    #[test]
    fn an_org_db_from_before_the_log_with_its_identity_row_still_counts_as_committed() {
        let dir = TempDir::new().unwrap();
        let store = Store::open(
            &dir.path().join("org.db"),
            &Key::from_bytes([5; 32]),
            Opening::New,
            &SCHEMA,
        )
        .unwrap();
        assert!(
            !store.read(is_committed).unwrap(),
            "a schema alone is not an organization"
        );
        store
            .write(|db| {
                db.execute(
                    "INSERT INTO meta (key, value) VALUES ('organization_id', 'ORG1')",
                    (),
                )
                .map_err(OfficeError::storage)?;
                Ok(((), None))
            })
            .unwrap();
        assert!(store.read(is_committed).unwrap());
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

    #[test]
    fn an_answer_is_kept_until_its_ids_time_plus_30_days_whatever_the_clock_says() {
        let (_dir, store) = store();
        let clock = 1_800_000_000_000u64;
        let made = clock - 86_400_000; // a day ago
        let id = crate::ulid::from_ms(made, 7);
        let command = OrgCommand::TeamCreate(TeamCreateParams {
            command_id: id.clone(),
            team_id: "T1".into(),
            name: "Platform".into(),
            owner_team_id: None,
        });

        store
            .write(move |db| Ok((submit(db, clock, &command).unwrap().unwrap(), None)))
            .unwrap();

        let (expires_at, epoch, seq): (String, i64, i64) = store
            .read(|db| {
                db.query_row(
                    "SELECT expires_at, epoch, seq FROM commands WHERE command_id = ?1",
                    [&id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .map_err(OfficeError::storage)
            })
            .unwrap();
        assert_eq!(
            expires_at,
            crate::time::format_epoch_ms(made + 30 * 86_400_000)
        );
        assert_eq!((epoch, seq), (1, 2));
    }

    fn create_team(id: &str) -> OrgCommand {
        OrgCommand::TeamCreate(TeamCreateParams {
            command_id: id.into(),
            team_id: "T1".into(),
            name: "Platform".into(),
            owner_team_id: None,
        })
    }

    fn run(store: &Store, clock: u64, command: OrgCommand) -> Result<CommandAccepted, ApiError> {
        store
            .write(move |db| Ok((submit(db, clock, &command).unwrap(), None)))
            .unwrap()
    }

    fn stored_commands(store: &Store) -> i64 {
        store
            .read(|db| {
                db.query_row("SELECT COUNT(*) FROM commands", (), |row| row.get(0))
                    .map_err(OfficeError::storage)
            })
            .unwrap()
    }

    #[test]
    fn the_window_edges_are_exactly_29_days_back_and_5_minutes_ahead() {
        let clock = 1_800_000_000_000u64;
        let ulid = |ms, n| crate::ulid::from_ms(ms, n);
        let day = 86_400_000;
        let code = |result: Result<CommandAccepted, ApiError>| result.unwrap_err().code;
        let (_dir, store) = store();

        // 29 days back to the millisecond is still inside; one more is `expired`.
        assert_eq!(
            code(run(
                &store,
                clock,
                create_team(&ulid(clock - 29 * day - 1, 1))
            )),
            ErrorCode::Expired
        );
        assert!(run(&store, clock, create_team(&ulid(clock - 29 * day, 2))).is_ok());
        // 5 minutes ahead exactly is inside; one more is `clock_skew`.
        let other = |id: &str| {
            OrgCommand::TeamCreate(TeamCreateParams {
                command_id: id.into(),
                team_id: "T2".into(),
                name: "Design".into(),
                owner_team_id: None,
            })
        };
        assert_eq!(
            code(run(&store, clock, other(&ulid(clock + 300_000 + 1, 3)))),
            ErrorCode::ClockSkew
        );
        assert!(run(&store, clock, other(&ulid(clock + 300_000, 4))).is_ok());
    }

    #[test]
    fn what_is_not_an_answer_is_never_stored() {
        let clock = 1_800_000_000_000u64;
        let (_dir, store) = store();
        for bad in [
            "not a ulid".to_owned(),
            crate::ulid::from_ms(clock - 40 * 86_400_000, 1),
            crate::ulid::from_ms(clock + 3_600_000, 2),
        ] {
            assert!(run(&store, clock, create_team(&bad)).is_err());
        }
        assert_eq!(stored_commands(&store), 0);

        // A refusal by the rules is an answer, and a commit is one.
        let refused = OrgCommand::TeamCreate(TeamCreateParams {
            command_id: crate::ulid::from_ms(clock, 3),
            team_id: "T1".into(),
            name: " ".into(),
            owner_team_id: None,
        });
        assert!(run(&store, clock, refused).is_err());
        assert!(run(&store, clock, create_team(&crate::ulid::from_ms(clock, 4))).is_ok());
        assert_eq!(stored_commands(&store), 2);
    }

    #[test]
    fn an_unreadable_clock_mark_is_an_error_not_no_mark() {
        let clock = 1_800_000_000_000u64;
        let (_dir, store) = store();
        store.run_sql_for_test("INSERT INTO meta (key, value) VALUES ('clock_high_ms', 'garbage')");

        let outcome = store.write(move |db| {
            let id = crate::ulid::from_ms(clock, 1);
            Ok((submit(db, clock, &create_team(&id)).is_err(), None))
        });

        assert!(outcome.unwrap());
        assert_eq!(stored_commands(&store), 0);
    }
}
