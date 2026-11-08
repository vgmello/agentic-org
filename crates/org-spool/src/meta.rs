//! Per-agent `flock`-protected metadata (`profile.json`, `status.json`, `triage.json`),
//! directory (`who`), agent status (`status`), and message triage (`defer`, `done`, `notify`, `due`).

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io;
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::io::AsRawFd;
use std::path::PathBuf;
use std::process;
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::envelope::{validate_agent_id, Envelope};
use crate::spool::{Spool, SpoolError};
use crate::time::{format_iso8601_utc, is_valid_iso8601_utc, parse_duration_arg};
use crate::ulid::{generate_ulid, is_valid_ulid};

const LOCK_EX: i32 = 2;
const LOCK_UN: i32 = 8;
const DEFAULT_STATUS_TTL_SECS: u64 = 8 * 3600;
const MAX_STATUS_TTL_SECS: u64 = 24 * 3600;
const MAX_STATUS_CHARS: usize = 140;
const MAX_STATUS_BYTES: usize = 256;
const MAX_NOTE_BYTES: usize = 1024;
const MAX_REPORTED_SESSIONS: usize = 16;

extern "C" {
    fn flock(fd: i32, operation: i32) -> i32;
}

struct MetaLockGuard {
    file: File,
}

impl Drop for MetaLockGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = flock(self.file.as_raw_fd(), LOCK_UN);
        }
    }
}

fn default_member_kind() -> String {
    "agent".to_string()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamEntry {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub title_in_team: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RefEntry {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemberProfile {
    pub id: String,
    pub name: String,
    #[serde(default = "default_member_kind")]
    pub kind: String,
    #[serde(default)]
    pub owner: bool,
    #[serde(default)]
    pub role_title: Option<String>,
    #[serde(default)]
    pub teams: Vec<TeamEntry>,
    #[serde(default)]
    pub reports_to: Option<RefEntry>,
    #[serde(default)]
    pub office: Option<RefEntry>,
}

impl MemberProfile {
    pub fn default_for(id: &str) -> Self {
        let is_owner = id.eq_ignore_ascii_case("vgmello") || id.eq_ignore_ascii_case("owner");
        Self {
            id: id.to_string(),
            name: id.to_string(),
            kind: if is_owner {
                "person".to_string()
            } else {
                "agent".to_string()
            },
            owner: is_owner,
            role_title: None,
            teams: Vec::new(),
            reports_to: None,
            office: Some(RefEntry {
                id: "hq".to_string(),
                name: "HQ".to_string(),
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemberStatus {
    pub status: String,
    pub updated_at: String,
    pub expires_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirectoryCard {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub owner: bool,
    pub role_title: Option<String>,
    pub teams: Vec<TeamEntry>,
    pub reports_to: Option<RefEntry>,
    pub office: Option<RefEntry>,
    pub presence: String,
    pub status: Option<String>,
    pub status_expires_at: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageTriageEntry {
    #[serde(default)]
    pub triage_gen: u64,
    #[serde(default)]
    pub snoozed_until: Option<String>,
    #[serde(default)]
    pub snooze_note: Option<String>,
    #[serde(default)]
    pub triaged_at: Option<String>,
    #[serde(default)]
    pub done_at: Option<String>,
    #[serde(default)]
    pub done_note: Option<String>,
    #[serde(default)]
    pub last_due_reason: Option<String>,
    #[serde(default)]
    pub reported_sessions: BTreeMap<String, u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TriageState {
    #[serde(default = "default_triage_version")]
    pub v: u32,
    #[serde(default)]
    pub messages: BTreeMap<String, MessageTriageEntry>,
}

fn default_triage_version() -> u32 {
    1
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeferOutcome {
    pub id: String,
    pub status: String,
    pub triage_gen: u64,
    pub snoozed_until: String,
    pub snooze_note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DoneOutcome {
    pub id: String,
    pub status: String,
    pub done_at: String,
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InboxItem {
    pub v: u32,
    pub id: String,
    pub from: String,
    pub to: String,
    pub kind: String,
    pub priority: String,
    pub urgent: bool,
    pub triage_gen: u64,
    pub status: String,
    pub snoozed_until: Option<String>,
    pub snooze_note: Option<String>,
    pub reply_to: Option<String>,
    pub created_at: String,
    pub expires_at: Option<String>,
    pub subject: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub meta: BTreeMap<String, Value>,
}

#[derive(Debug, Clone)]
pub struct NotifyOptions {
    pub session: Option<String>,
    pub after_task: bool,
    pub urgent_only: bool,
    pub sweep_after: Duration,
}

impl Default for NotifyOptions {
    fn default() -> Self {
        Self {
            session: None,
            after_task: false,
            urgent_only: false,
            sweep_after: Duration::from_secs(5 * 60),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotifyItem {
    pub id: String,
    pub from: String,
    pub from_name: String,
    pub subject: String,
    pub kind: String,
    pub urgent: bool,
    pub priority: String,
    pub triage_gen: u64,
    pub created_at: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotifyReport {
    pub v: u32,
    pub agent: String,
    pub urgent: bool,
    pub count: usize,
    pub urgent_count: usize,
    pub items: Vec<NotifyItem>,
    pub more: usize,
    pub text: String,
}

pub fn sanitize_inline_text(raw: &str, max_chars: usize) -> String {
    let mut cleaned = String::with_capacity(raw.len().min(max_chars + 8));
    let mut prev_space = false;
    for ch in raw.chars() {
        let mapped = if ch.is_control() { ' ' } else { ch };
        if mapped.is_whitespace() {
            if !prev_space && !cleaned.is_empty() {
                cleaned.push(' ');
            }
            prev_space = true;
        } else {
            cleaned.push(mapped);
            prev_space = false;
        }
    }
    let trimmed = cleaned.trim();
    if trimmed.chars().count() <= max_chars {
        trimmed.to_string()
    } else {
        let mut out: String = trimmed.chars().take(max_chars.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

pub fn parse_snooze_until(raw: &str, now: SystemTime) -> Result<String, SpoolError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(SpoolError::InvalidInput(
            "--until requires a duration (e.g. 30m, 2h), 'after-task', or an ISO-8601 UTC timestamp"
                .into(),
        ));
    }
    if trimmed == "after-task" {
        return Ok("after-task".to_string());
    }
    if is_valid_iso8601_utc(trimmed) {
        return Ok(trimmed.to_string());
    }
    // Also tolerate YYYY-MM-DDTHH:MM:SSZ without fractional seconds by normalizing to .000Z
    if trimmed.len() == 20 && trimmed.ends_with('Z') {
        let normalized = format!("{}.000Z", &trimmed[..19]);
        if is_valid_iso8601_utc(&normalized) {
            return Ok(normalized);
        }
    }
    let dur = parse_duration_arg(trimmed).map_err(SpoolError::InvalidInput)?;
    let target = now.checked_add(dur).unwrap_or(now);
    Ok(format_iso8601_utc(target))
}

impl Spool {
    fn agent_meta_lock_path(&self, agent: &str) -> PathBuf {
        self.agent_inbox_dir(agent).join(".meta.lock")
    }

    fn agent_profile_path(&self, agent: &str) -> PathBuf {
        self.agent_inbox_dir(agent).join("profile.json")
    }

    fn agent_status_path(&self, agent: &str) -> PathBuf {
        self.agent_inbox_dir(agent).join("status.json")
    }

    fn agent_triage_path(&self, agent: &str) -> PathBuf {
        self.agent_inbox_dir(agent).join("triage.json")
    }

    /// Acquire an exclusive `flock` on `inbox/<agent>/.meta.lock` and execute `f`.
    pub fn with_agent_meta_lock<T, F>(&self, agent: &str, f: F) -> Result<T, SpoolError>
    where
        F: FnOnce() -> Result<T, SpoolError>,
    {
        validate_agent_id(agent, "agent").map_err(SpoolError::InvalidInput)?;
        self.ensure_dir(&self.agent_tmp_dir(agent))?;
        let lock_path = self.agent_meta_lock_path(agent);
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(&lock_path)?;
        loop {
            let rc = unsafe { flock(file.as_raw_fd(), LOCK_EX) };
            if rc == 0 {
                break;
            }
            let err = io::Error::last_os_error();
            if err.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(SpoolError::Io(err));
        }
        let _guard = MetaLockGuard { file };
        f()
    }

    fn write_agent_json_unlocked<T: Serialize>(
        &self,
        agent: &str,
        dest: &std::path::Path,
        value: &T,
    ) -> Result<(), SpoolError> {
        let bytes = serde_json::to_vec_pretty(value)
            .map_err(|e| SpoolError::InvalidInput(format!("serialize metadata: {e}")))?;
        self.ensure_dir(&self.agent_tmp_dir(agent))?;
        let tmp_name = format!(".meta-{}-{}.tmp", process::id(), generate_ulid());
        let tmp_path = self.agent_tmp_dir(agent).join(tmp_name);
        Self::write_file_0600(&tmp_path, &bytes, true)?;
        fs::rename(&tmp_path, dest)?;
        Ok(())
    }

    fn load_triage_unlocked(&self, agent: &str) -> TriageState {
        let path = self.agent_triage_path(agent);
        let Ok(bytes) = fs::read(&path) else {
            return TriageState {
                v: 1,
                messages: BTreeMap::new(),
            };
        };
        serde_json::from_slice(&bytes).unwrap_or_else(|_| TriageState {
            v: 1,
            messages: BTreeMap::new(),
        })
    }

    fn save_triage_unlocked(&self, agent: &str, state: &TriageState) -> Result<(), SpoolError> {
        self.write_agent_json_unlocked(agent, &self.agent_triage_path(agent), state)
    }

    fn load_profile_unlocked(&self, agent: &str) -> MemberProfile {
        let path = self.agent_profile_path(agent);
        if let Ok(bytes) = fs::read(&path) {
            if let Ok(profile) = serde_json::from_slice::<MemberProfile>(&bytes) {
                return profile;
            }
        }
        MemberProfile::default_for(agent)
    }

    /// Update or create `inbox/<agent>/profile.json` under the per-agent `flock`.
    pub fn set_profile(&self, profile: &MemberProfile) -> Result<MemberProfile, SpoolError> {
        validate_agent_id(&profile.id, "id").map_err(SpoolError::InvalidInput)?;
        self.register(&profile.id)?;
        self.with_agent_meta_lock(&profile.id, || {
            self.write_agent_json_unlocked(
                &profile.id,
                &self.agent_profile_path(&profile.id),
                profile,
            )?;
            Ok(profile.clone())
        })
    }

    /// Read a member's profile (falling back to default profile if not customized).
    pub fn get_profile(&self, agent: &str) -> Result<MemberProfile, SpoolError> {
        validate_agent_id(agent, "agent").map_err(SpoolError::InvalidInput)?;
        Ok(self.load_profile_unlocked(agent))
    }

    /// Set the calling agent's status line (`inbox/<agent>/status.json`) with default 8h TTL.
    pub fn set_status(
        &self,
        agent: &str,
        text: &str,
        ttl: Option<Duration>,
    ) -> Result<MemberStatus, SpoolError> {
        validate_agent_id(agent, "agent").map_err(SpoolError::InvalidInput)?;
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return Err(SpoolError::InvalidInput(
                "status text cannot be empty (use --clear to clear status)".into(),
            ));
        }
        if trimmed.chars().any(|c| c.is_control()) {
            return Err(SpoolError::InvalidInput(
                "status text cannot contain control characters or newlines".into(),
            ));
        }
        if trimmed.chars().count() > MAX_STATUS_CHARS || trimmed.len() > MAX_STATUS_BYTES {
            return Err(SpoolError::InvalidInput(format!(
                "status text exceeds limit ({MAX_STATUS_CHARS} chars / {MAX_STATUS_BYTES} bytes)"
            )));
        }
        let ttl_dur = ttl.unwrap_or(Duration::from_secs(DEFAULT_STATUS_TTL_SECS));
        if ttl_dur.is_zero() || ttl_dur > Duration::from_secs(MAX_STATUS_TTL_SECS) {
            return Err(SpoolError::InvalidInput(
                "status TTL must be between 1s and 24h".into(),
            ));
        }
        self.register(agent)?;
        let now = SystemTime::now();
        let expires = now.checked_add(ttl_dur).unwrap_or(now);
        let status = MemberStatus {
            status: trimmed.to_string(),
            updated_at: format_iso8601_utc(now),
            expires_at: format_iso8601_utc(expires),
        };
        self.with_agent_meta_lock(agent, || {
            self.write_agent_json_unlocked(agent, &self.agent_status_path(agent), &status)?;
            Ok(status)
        })
    }

    /// Clear an agent's status line.
    pub fn clear_status(&self, agent: &str) -> Result<(), SpoolError> {
        validate_agent_id(agent, "agent").map_err(SpoolError::InvalidInput)?;
        if !self.agent_inbox_dir(agent).exists() {
            return Ok(());
        }
        self.with_agent_meta_lock(agent, || {
            let path = self.agent_status_path(agent);
            if path.exists() {
                let _ = fs::remove_file(&path);
            }
            Ok(())
        })
    }

    /// Return an agent's active (non-expired) status line, or `None` if unset/expired.
    pub fn get_active_status(&self, agent: &str, now_iso: &str) -> Option<MemberStatus> {
        let path = self.agent_status_path(agent);
        let bytes = fs::read(&path).ok()?;
        let status: MemberStatus = serde_json::from_slice(&bytes).ok()?;
        if status.expires_at.as_str() <= now_iso {
            None
        } else {
            Some(status)
        }
    }

    fn build_directory_card(&self, id: &str, now_iso: &str) -> DirectoryCard {
        let profile = self.load_profile_unlocked(id);
        let active_status = self.get_active_status(id, now_iso);
        let has_live_waiter = self.has_live_wake_listener(id);
        let presence = if has_live_waiter {
            "idle".to_string()
        } else if active_status.is_some() || self.is_registered(id) {
            "online".to_string()
        } else {
            "offline".to_string()
        };
        DirectoryCard {
            id: profile.id,
            name: profile.name,
            kind: profile.kind,
            owner: profile.owner,
            role_title: profile.role_title,
            teams: profile.teams,
            reports_to: profile.reports_to,
            office: profile.office,
            presence,
            status: active_status.as_ref().map(|s| s.status.clone()),
            status_expires_at: active_status.map(|s| s.expires_at),
        }
    }

    /// List all members in the directory (`org-agent who`), optionally filtered by `--team` and `--kind`.
    pub fn list_directory(
        &self,
        team_filter: Option<&str>,
        kind_filter: Option<&str>,
    ) -> Result<Vec<DirectoryCard>, SpoolError> {
        let now_iso = format_iso8601_utc(SystemTime::now());
        let mut ids = self.status()?.registered_agents;
        if !ids.iter().any(|x| x == "vgmello") {
            ids.push("vgmello".to_string());
        }
        ids.sort();
        ids.dedup();

        let mut cards = Vec::new();
        for id in ids {
            let card = self.build_directory_card(&id, &now_iso);
            if let Some(kf) = kind_filter {
                let kf_lower = kf.trim().to_ascii_lowercase();
                if kf_lower == "owner" {
                    if !card.owner {
                        continue;
                    }
                } else if !card.kind.eq_ignore_ascii_case(&kf_lower) {
                    continue;
                }
            }
            if let Some(tf) = team_filter {
                let tf_lower = tf.trim().to_ascii_lowercase();
                let in_team = card.teams.iter().any(|t| {
                    t.id.eq_ignore_ascii_case(&tf_lower) || t.name.eq_ignore_ascii_case(&tf_lower)
                });
                if !in_team {
                    continue;
                }
            }
            cards.push(card);
        }
        Ok(cards)
    }

    /// Resolve a single member card by ID, display name, or `"owner"`.
    pub fn resolve_directory_member(&self, query: &str) -> Result<DirectoryCard, SpoolError> {
        let q = query.trim();
        if q.is_empty() {
            return Err(SpoolError::InvalidInput(
                "member name or id cannot be empty".into(),
            ));
        }
        let all = self.list_directory(None, None)?;
        if q.eq_ignore_ascii_case("owner") {
            if let Some(owner_card) = all.iter().find(|c| c.owner).cloned() {
                return Ok(owner_card);
            }
        }
        if let Some(exact_id) = all.iter().find(|c| c.id.eq_ignore_ascii_case(q)) {
            return Ok(exact_id.clone());
        }
        let name_matches: Vec<DirectoryCard> = all
            .into_iter()
            .filter(|c| c.name.eq_ignore_ascii_case(q))
            .collect();
        if name_matches.len() == 1 {
            return Ok(name_matches.into_iter().next().unwrap());
        }
        if name_matches.len() > 1 {
            return Err(SpoolError::InvalidInput(format!(
                "ambiguous member name '{q}'; specify the member id"
            )));
        }
        Err(SpoolError::NotFound(format!("member '{q}' not found")))
    }

    /// Snooze (`defer`) a message `<id>` until `until_arg` (`30m`, `2h`, `after-task`, or ISO-8601 UTC).
    /// If the message was already moved to `cur/` by `msg read <id>`, moves it back to `new/` so it
    /// stays unread and resurfaces when the snooze becomes due.
    pub fn defer_message(
        &self,
        agent: &str,
        id: &str,
        until_arg: &str,
        note: Option<String>,
    ) -> Result<DeferOutcome, SpoolError> {
        validate_agent_id(agent, "agent").map_err(SpoolError::InvalidInput)?;
        if !is_valid_ulid(id) {
            return Err(SpoolError::InvalidInput(format!(
                "invalid message id '{id}'"
            )));
        }
        if let Some(ref n) = note {
            if n.len() > MAX_NOTE_BYTES {
                return Err(SpoolError::InvalidInput(format!(
                    "note exceeds maximum {MAX_NOTE_BYTES} bytes"
                )));
            }
        }
        let now = SystemTime::now();
        let now_iso = format_iso8601_utc(now);
        let snoozed_until = parse_snooze_until(until_arg, now)?;

        self.with_agent_meta_lock(agent, || {
            let fname = format!("{id}.json");
            let new_path = self.agent_new_dir(agent).join(&fname);
            let cur_path = self.agent_cur_dir(agent).join(&fname);

            if !new_path.is_file() {
                if cur_path.is_file() {
                    self.ensure_dir(&self.agent_new_dir(agent))?;
                    fs::rename(&cur_path, &new_path)?;
                } else {
                    return Err(SpoolError::NotFound(format!(
                        "message '{id}' not found in inbox for '{agent}'"
                    )));
                }
            }

            let mut state = self.load_triage_unlocked(agent);
            let entry = state.messages.entry(id.to_string()).or_default();
            entry.triage_gen = entry.triage_gen.saturating_add(1);
            entry.snoozed_until = Some(snoozed_until.clone());
            entry.snooze_note = note.clone();
            entry.triaged_at = Some(now_iso);
            entry.done_at = None;
            entry.done_note = None;
            entry.last_due_reason = None;
            entry.reported_sessions.clear();
            let gen = entry.triage_gen;

            self.save_triage_unlocked(agent, &state)?;
            Ok(DeferOutcome {
                id: id.to_string(),
                status: "snoozed".to_string(),
                triage_gen: gen,
                snoozed_until,
                snooze_note: note,
            })
        })
    }

    /// Mark a message `<id>` done without a reply (`new/` -> `cur/` and recorded in `triage.json`).
    pub fn done_message(
        &self,
        agent: &str,
        id: &str,
        note: Option<String>,
    ) -> Result<DoneOutcome, SpoolError> {
        validate_agent_id(agent, "agent").map_err(SpoolError::InvalidInput)?;
        if !is_valid_ulid(id) {
            return Err(SpoolError::InvalidInput(format!(
                "invalid message id '{id}'"
            )));
        }
        if let Some(ref n) = note {
            if n.len() > MAX_NOTE_BYTES {
                return Err(SpoolError::InvalidInput(format!(
                    "note exceeds maximum {MAX_NOTE_BYTES} bytes"
                )));
            }
        }
        let now_iso = format_iso8601_utc(SystemTime::now());

        self.with_agent_meta_lock(agent, || {
            let fname = format!("{id}.json");
            let new_path = self.agent_new_dir(agent).join(&fname);
            let cur_path = self.agent_cur_dir(agent).join(&fname);

            if new_path.is_file() {
                self.ensure_dir(&self.agent_cur_dir(agent))?;
                match fs::rename(&new_path, &cur_path) {
                    Ok(()) => {}
                    Err(e) if e.kind() == io::ErrorKind::NotFound && cur_path.is_file() => {}
                    Err(e) => return Err(SpoolError::Io(e)),
                }
            } else if !cur_path.is_file() {
                return Err(SpoolError::NotFound(format!(
                    "message '{id}' not found in inbox for '{agent}'"
                )));
            }

            let mut state = self.load_triage_unlocked(agent);
            let entry = state.messages.entry(id.to_string()).or_default();
            entry.snoozed_until = None;
            entry.last_due_reason = None;
            entry.done_at = Some(now_iso.clone());
            entry.done_note = note.clone();
            entry.triaged_at = Some(now_iso.clone());
            entry.reported_sessions.clear();

            self.save_triage_unlocked(agent, &state)?;
            Ok(DoneOutcome {
                id: id.to_string(),
                status: "done".to_string(),
                done_at: now_iso,
                note,
            })
        })
    }

    fn release_due_in_state(state: &mut TriageState, now_iso: &str, after_task: bool) -> bool {
        let mut modified = false;
        for entry in state.messages.values_mut() {
            let Some(ref until) = entry.snoozed_until else {
                continue;
            };
            if until == "after-task" {
                if after_task {
                    entry.snoozed_until = None;
                    entry.triage_gen = entry.triage_gen.saturating_add(1);
                    entry.triaged_at = Some(now_iso.to_string());
                    entry.last_due_reason = Some("after_task".to_string());
                    entry.reported_sessions.clear();
                    modified = true;
                }
            } else if until.as_str() <= now_iso {
                entry.snoozed_until = None;
                entry.triage_gen = entry.triage_gen.saturating_add(1);
                entry.triaged_at = Some(now_iso.to_string());
                entry.last_due_reason = Some("due".to_string());
                entry.reported_sessions.clear();
                modified = true;
            }
        }
        modified
    }

    /// Release any time-based snoozes whose deadline `<= now` (and `after-task` snoozes if `after_task` is true).
    pub fn release_due_snoozes(
        &self,
        agent: &str,
        after_task: bool,
    ) -> Result<TriageState, SpoolError> {
        validate_agent_id(agent, "agent").map_err(SpoolError::InvalidInput)?;
        if !self.agent_inbox_dir(agent).exists() {
            return Ok(TriageState::default());
        }
        let now_iso = format_iso8601_utc(SystemTime::now());
        self.with_agent_meta_lock(agent, || {
            let mut state = self.load_triage_unlocked(agent);
            if Self::release_due_in_state(&mut state, &now_iso, after_task) {
                self.save_triage_unlocked(agent, &state)?;
            }
            Ok(state)
        })
    }

    /// Clear any active snooze on `id` when the message is explicitly read or replied to.
    pub(crate) fn mark_read_in_triage(&self, agent: &str, id: &str) {
        if !self.agent_inbox_dir(agent).exists() {
            return;
        }
        let now_iso = format_iso8601_utc(SystemTime::now());
        let _ = self.with_agent_meta_lock(agent, || {
            let mut state = self.load_triage_unlocked(agent);
            if let Some(entry) = state.messages.get_mut(id) {
                if entry.snoozed_until.is_some() || entry.last_due_reason.is_some() {
                    entry.snoozed_until = None;
                    entry.last_due_reason = None;
                    entry.triaged_at = Some(now_iso);
                    self.save_triage_unlocked(agent, &state)?;
                }
            }
            Ok(())
        });
    }

    /// List inbox items with triage metadata, hiding currently snoozed messages unless `include_all` is true.
    pub fn list_inbox_items(
        &self,
        agent: &str,
        include_all: bool,
        summary_only: bool,
        limit: Option<usize>,
    ) -> Result<Vec<InboxItem>, SpoolError> {
        validate_agent_id(agent, "agent").map_err(SpoolError::InvalidInput)?;
        let state = self.release_due_snoozes(agent, false)?;
        let new_envs = self.list_inbox(agent, false)?;
        let mut items = Vec::new();

        for env in new_envs {
            let t = state.messages.get(&env.id).cloned().unwrap_or_default();
            if t.done_at.is_some() && !include_all {
                continue;
            }
            let is_snoozed = t.snoozed_until.is_some();
            if is_snoozed && !include_all {
                continue;
            }
            let status = if t.done_at.is_some() {
                "done".to_string()
            } else if is_snoozed {
                "snoozed".to_string()
            } else {
                "unread".to_string()
            };
            items.push(InboxItem {
                v: env.v,
                id: env.id.clone(),
                from: env.from.clone(),
                to: env.to.clone(),
                kind: env.kind.as_str().to_string(),
                priority: env.effective_priority().as_str().to_string(),
                urgent: env.is_urgent(),
                triage_gen: t.triage_gen,
                status,
                snoozed_until: t.snoozed_until,
                snooze_note: t.snooze_note,
                reply_to: env.reply_to.clone(),
                created_at: env.created_at.clone(),
                expires_at: env.expires_at.clone(),
                subject: env.subject.clone(),
                body: if summary_only { None } else { Some(env.body) },
                meta: if summary_only {
                    BTreeMap::new()
                } else {
                    env.meta
                },
            });
        }

        if include_all {
            for path in Self::sorted_json_entries(&self.agent_cur_dir(agent))? {
                if let Ok(bytes) = fs::read(&path) {
                    if let Ok(env) = Envelope::from_json_bytes(&bytes) {
                        if items.iter().any(|i| i.id == env.id) {
                            continue;
                        }
                        let t = state.messages.get(&env.id).cloned().unwrap_or_default();
                        let status = if t.done_at.is_some() {
                            "done".to_string()
                        } else {
                            "read".to_string()
                        };
                        items.push(InboxItem {
                            v: env.v,
                            id: env.id.clone(),
                            from: env.from.clone(),
                            to: env.to.clone(),
                            kind: env.kind.as_str().to_string(),
                            priority: env.effective_priority().as_str().to_string(),
                            urgent: env.is_urgent(),
                            triage_gen: t.triage_gen,
                            status,
                            snoozed_until: t.snoozed_until,
                            snooze_note: t.snooze_note,
                            reply_to: env.reply_to.clone(),
                            created_at: env.created_at.clone(),
                            expires_at: env.expires_at.clone(),
                            subject: env.subject.clone(),
                            body: if summary_only { None } else { Some(env.body) },
                            meta: if summary_only {
                                BTreeMap::new()
                            } else {
                                env.meta
                            },
                        });
                    }
                }
            }
            items.sort_by(|a, b| a.id.cmp(&b.id));
        }

        if let Some(lim) = limit {
            items.truncate(lim);
        }
        Ok(items)
    }

    /// Evaluate actionable unread messages and build the sanitized `NotifyReport` (`org-agent msg notify`).
    pub fn notify(&self, agent: &str, opts: &NotifyOptions) -> Result<NotifyReport, SpoolError> {
        validate_agent_id(agent, "agent").map_err(SpoolError::InvalidInput)?;
        if !self.agent_inbox_dir(agent).exists() {
            return Ok(NotifyReport {
                v: 1,
                agent: agent.to_string(),
                urgent: false,
                count: 0,
                urgent_count: 0,
                items: Vec::new(),
                more: 0,
                text: String::new(),
            });
        }

        let now = SystemTime::now();
        let now_iso = format_iso8601_utc(now);
        let sweep_cutoff_iso = format_iso8601_utc(
            now.checked_sub(opts.sweep_after)
                .unwrap_or(SystemTime::UNIX_EPOCH),
        );

        let items = self.with_agent_meta_lock(agent, || {
            let mut state = self.load_triage_unlocked(agent);
            let mut state_dirty = Self::release_due_in_state(&mut state, &now_iso, opts.after_task);
            let unread_envs = self.list_inbox(agent, false)?;
            let mut matched = Vec::new();

            for env in unread_envs {
                let entry = state.messages.entry(env.id.clone()).or_default();
                if entry.done_at.is_some() || entry.snoozed_until.is_some() {
                    continue;
                }
                let urgent = env.is_urgent();
                if opts.urgent_only && !urgent {
                    continue;
                }
                let priority = env.effective_priority().as_str().to_string();

                let reason = if let Some(ref due_reason) = entry.last_due_reason {
                    Some(due_reason.clone())
                } else if urgent || opts.after_task {
                    Some("new".to_string())
                } else if entry.triaged_at.is_none()
                    && env.created_at.as_str() <= sweep_cutoff_iso.as_str()
                {
                    Some("sweep".to_string())
                } else {
                    None
                };

                let Some(reason) = reason else {
                    continue;
                };

                if let Some(ref session_id) = opts.session {
                    let trimmed_sess = session_id.trim();
                    if !trimmed_sess.is_empty() {
                        if entry.reported_sessions.get(trimmed_sess) == Some(&entry.triage_gen) {
                            continue;
                        }
                        if entry.reported_sessions.len() >= MAX_REPORTED_SESSIONS {
                            if let Some(first_key) = entry.reported_sessions.keys().next().cloned()
                            {
                                entry.reported_sessions.remove(&first_key);
                            }
                        }
                        entry
                            .reported_sessions
                            .insert(trimmed_sess.to_string(), entry.triage_gen);
                        state_dirty = true;
                    }
                }

                let from_profile = self.load_profile_unlocked(&env.from);
                let from_name = sanitize_inline_text(&from_profile.name, 48);
                let from_id = sanitize_inline_text(&env.from, 64);
                let raw_subj = env.subject.as_deref().unwrap_or("(no subject)");
                let clean_subj = {
                    let s = sanitize_inline_text(raw_subj, 80);
                    if s.is_empty() {
                        "(no subject)".to_string()
                    } else {
                        s
                    }
                };

                matched.push(NotifyItem {
                    id: env.id,
                    from: from_id,
                    from_name,
                    subject: clean_subj,
                    kind: env.kind.as_str().to_string(),
                    urgent,
                    priority,
                    triage_gen: entry.triage_gen,
                    created_at: env.created_at,
                    reason,
                });
            }

            if state_dirty {
                self.save_triage_unlocked(agent, &state)?;
            }
            Ok(matched)
        })?;

        let count = items.len();
        let urgent_count = items.iter().filter(|i| i.urgent).count();
        let urgent = urgent_count > 0;
        let more = count.saturating_sub(10);

        let text = if count == 0 {
            String::new()
        } else {
            let mut lines = Vec::new();
            if urgent_count > 0 {
                lines.push(format!("{count} new message(s), {urgent_count} urgent:"));
            } else {
                lines.push(format!("{count} new message(s):"));
            }
            for item in items.iter().take(10) {
                let prefix = if item.urgent { "- [URGENT] " } else { "- " };
                lines.push(format!(
                    "{prefix}{} ({}): {} [{}] ({})",
                    item.from_name, item.from, item.subject, item.kind, item.id
                ));
            }
            if more > 0 {
                lines.push(format!("…and {more} more (org-agent msg inbox)"));
            }
            lines.push(
                "Triage each: answer now, `org-agent msg defer <id> --until 30m|2h|after-task|<time>`, or `org-agent msg done <id>`."
                    .to_string(),
            );
            lines.join("\n")
        };

        Ok(NotifyReport {
            v: 1,
            agent: agent.to_string(),
            urgent,
            count,
            urgent_count,
            items,
            more,
            text,
        })
    }
}
