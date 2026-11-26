//! Organization-level `L4` provider connection & ordered model profile pool mutations
//! (`TASK-54`, `FC-098`, `doc-88` §4, `doc-89`).

use org_store::{Actor, Change, Engine, Id, OfficeIdentity, Refusal, UnitOfWork};
use serde::Serialize;

use super::shared::{admit, id_in_use, reject};
use super::{changed, invalid, stable, Feature, Op, RejectCode};
use crate::organization::ports::OrganizationRead;
use crate::organization::ports::TeamDependents;
use crate::organization::records::{self, Meta, ModelProfile};

fn revision_matches(actual: u64, expected: u64) -> Result<(), Refusal> {
    if actual != expected {
        return reject(RejectCode::Conflict, "stale revision");
    }
    Ok(())
}

fn bump_meta(meta: &mut Meta, by: &records::By, at: org_store::Timestamp) {
    meta.revision = meta.revision.saturating_add(1);
    meta.updated_at = Some(records::time_text(at));
    meta.updated_by = by.clone();
}

/// Validates an optional provider `endpoint_url`: must be a single line, `https://` or
/// loopback `http://`, with no embedded credentials (`user:pass@`), backslashes, whitespace,
/// or malformed host/port (`TASK-54` SSRF/parser-confusion hardening).
pub fn validate_endpoint_url(url: &str) -> bool {
    if !records::is_line(url)
        || url.len() > 2048
        || url.contains('\\')
        || url.chars().any(char::is_whitespace)
    {
        return false;
    }
    let (is_http, rest) = if let Some(r) = url.strip_prefix("https://") {
        (false, r)
    } else if let Some(r) = url.strip_prefix("http://") {
        (true, r)
    } else {
        return false;
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.is_empty() || authority.contains('@') {
        return false;
    }
    let Some((host, port)) = split_authority_host_port(authority) else {
        return false;
    };
    if port.is_some_and(|p| p == 0) {
        return false;
    }
    if is_http {
        matches!(host, "localhost" | "127.0.0.1" | "::1")
    } else {
        !host.is_empty()
    }
}

fn split_authority_host_port(authority: &str) -> Option<(&str, Option<u16>)> {
    if let Some(after_open) = authority.strip_prefix('[') {
        let (ipv6, tail) = after_open.split_once(']')?;
        if ipv6.is_empty()
            || !ipv6
                .bytes()
                .all(|b| b.is_ascii_hexdigit() || b == b':' || b == b'.')
        {
            return None;
        }
        if tail.is_empty() {
            return Some((ipv6, None));
        }
        let port_str = tail.strip_prefix(':')?;
        let port = parse_valid_port(port_str)?;
        return Some((ipv6, Some(port)));
    }
    if authority.contains('[') || authority.contains(']') {
        return None;
    }
    let (host, port) = match authority.split_once(':') {
        Some((h, p)) => (h, Some(parse_valid_port(p)?)),
        None => (authority, None),
    };
    if host.is_empty()
        || !host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
    {
        return None;
    }
    Some((host, port))
}

fn parse_valid_port(raw: &str) -> Option<u16> {
    if raw.is_empty() || !raw.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let p: u16 = raw.parse().ok()?;
    (p >= 1).then_some(p)
}

/// Owner updates a provider connection's mutable fields (`name`, `endpoint_url`, `enabled`,
/// `archived`, `max_concurrent_turns`). `(provider_type, subscription_identity)` stay immutable.
#[derive(Clone, PartialEq, Eq, Debug, Serialize)]
pub struct UpdateProviderConnection {
    pub id: Id,
    pub expected_revision: u64,
    pub name: Option<String>,
    pub endpoint_url: Option<String>,
    pub clear_endpoint_url: bool,
    pub enabled: Option<bool>,
    pub archived: Option<bool>,
    pub max_concurrent_turns: Option<u64>,
}

impl Feature for UpdateProviderConnection {
    const OP: Op = Op::UpdateProviderConnection;

    fn decide<E: Engine>(
        &self,
        uow: &mut UnitOfWork<'_, E>,
        here: &OfficeIdentity,
        owner: &Id,
        by: &Actor,
    ) -> Result<Change, Refusal>
    where
        E::Tx: TeamDependents,
    {
        stable(&self.id, "the connection's ID")?;
        let admitted = admit(uow, here, owner, by)?;
        let view = uow.view();
        let Some(mut conn) = view.provider_connection(&self.id)? else {
            return reject(RejectCode::NotFound, "provider connection not found");
        };
        revision_matches(conn.meta.revision, self.expected_revision)?;
        validate_connection_update(view, &conn.id, self)?;
        if let Some(ref name) = self.name {
            conn.name = name.clone();
        }
        if self.clear_endpoint_url {
            conn.endpoint_url = None;
        } else if let Some(ref url) = self.endpoint_url {
            conn.endpoint_url = Some(url.clone());
        }
        if let Some(enabled) = self.enabled {
            conn.enabled = enabled;
        }
        if let Some(archived) = self.archived {
            conn.archived = archived;
            if archived {
                conn.enabled = false;
            }
        }
        if let Some(max_turns) = self.max_concurrent_turns {
            conn.max_concurrent_turns = max_turns;
        }
        bump_meta(&mut conn.meta, &admitted.by, admitted.now);
        let reply = changed(&conn.id, conn.meta.revision, admitted.generation);
        Ok(Change::new(Op::UpdateProviderConnection.ack())
            .put(
                records::provider_connection_path(&conn.id),
                records::encode_provider_connection(&conn),
            )
            .reply(reply))
    }
}

fn validate_connection_update(
    view: &impl TeamDependents,
    conn_id: &Id,
    cmd: &UpdateProviderConnection,
) -> Result<(), Refusal> {
    if cmd.clear_endpoint_url && cmd.endpoint_url.is_some() {
        return reject(
            RejectCode::InvalidRequest,
            "endpoint_url and clear_endpoint_url cannot both be set",
        );
    }
    if cmd.name.as_deref().is_some_and(|n| !records::is_line(n)) {
        return reject(RejectCode::InvalidName, "name must be a single line");
    }
    if cmd
        .endpoint_url
        .as_deref()
        .is_some_and(|u| !validate_endpoint_url(u))
    {
        return reject(
            RejectCode::InvalidRequest,
            "endpoint_url must be a valid https:// or loopback http:// URL without credentials",
        );
    }
    if cmd
        .max_concurrent_turns
        .is_some_and(|n| !(1..=1000).contains(&n))
    {
        return reject(
            RejectCode::InvalidRequest,
            "max_concurrent_turns must be between 1 and 1000",
        );
    }
    if cmd.archived == Some(true) {
        let profiles = view.model_profiles()?;
        let blocking: Vec<&str> = profiles
            .iter()
            .filter(|p| !p.archived && p.connection_ids.iter().any(|c| c == conn_id))
            .map(|p| p.id.as_str())
            .collect();
        if !blocking.is_empty() {
            return reject(
                RejectCode::Conflict,
                format!(
                    "CONNECTION_IN_USE: referenced by active profiles: {}",
                    blocking.join(", ")
                ),
            );
        }
    }
    Ok(())
}

/// Owner creates a manually assembled ordered model profile pool (`1..=64` distinct connections).
#[derive(Clone, PartialEq, Eq, Debug, Serialize)]
pub struct CreateModelProfile {
    pub id: Id,
    pub key: String,
    pub name: String,
    pub description: String,
    pub connection_ids: Vec<Id>,
}

impl Feature for CreateModelProfile {
    const OP: Op = Op::CreateModelProfile;

    fn decide<E: Engine>(
        &self,
        uow: &mut UnitOfWork<'_, E>,
        here: &OfficeIdentity,
        owner: &Id,
        by: &Actor,
    ) -> Result<Change, Refusal>
    where
        E::Tx: TeamDependents,
    {
        stable(&self.id, "the model profile's ID")?;
        for cid in &self.connection_ids {
            stable(cid, "connection_id")?;
        }
        let admitted = admit(uow, here, owner, by)?;
        let view = uow.view();
        if !records::is_line(&self.key) || !records::is_line(&self.name) {
            return reject(
                RejectCode::InvalidName,
                "key and name must be single lines of text",
            );
        }
        if id_in_use(view, &admitted.organization, &self.id)? {
            return reject(RejectCode::AlreadyExists, "model profile ID is in use");
        }
        if view.model_profiles()?.iter().any(|p| p.key == self.key) {
            return reject(
                RejectCode::AlreadyExists,
                "a model profile with that key already exists",
            );
        }
        validate_pool_connections(view, &self.connection_ids)?;
        let profile = ModelProfile {
            id: self.id.clone(),
            key: self.key.clone(),
            name: self.name.clone(),
            description: self.description.clone(),
            is_auto_generated: false,
            generated_for_connection_id: None,
            archived: false,
            connection_ids: self.connection_ids.clone(),
            meta: Meta::created(&admitted.by, admitted.now),
        };
        let reply = changed(&profile.id, profile.meta.revision, admitted.generation);
        Ok(Change::new(Op::CreateModelProfile.ack())
            .put(
                records::model_profile_path(&profile.id),
                records::encode_model_profile(&profile),
            )
            .reply(reply))
    }
}

/// Owner updates a model profile pool's name, description, archive flag, or ordered connections.
/// Replacing `connection_ids` on an auto-generated default profile clears `is_auto_generated`
/// and `generated_for_connection_id` atomically (`doc-88` §4, `doc-89`).
#[derive(Clone, PartialEq, Eq, Debug, Serialize)]
pub struct UpdateModelProfile {
    pub id: Id,
    pub expected_revision: u64,
    pub name: Option<String>,
    pub description: Option<String>,
    pub archived: Option<bool>,
    pub connection_ids: Option<Vec<Id>>,
}

impl Feature for UpdateModelProfile {
    const OP: Op = Op::UpdateModelProfile;

    fn decide<E: Engine>(
        &self,
        uow: &mut UnitOfWork<'_, E>,
        here: &OfficeIdentity,
        owner: &Id,
        by: &Actor,
    ) -> Result<Change, Refusal>
    where
        E::Tx: TeamDependents,
    {
        stable(&self.id, "the model profile's ID")?;
        if let Some(ref cids) = self.connection_ids {
            for cid in cids {
                stable(cid, "connection_id")?;
            }
        }
        let admitted = admit(uow, here, owner, by)?;
        let view = uow.view();
        let Some(mut profile) = view.model_profile(&self.id)? else {
            return reject(RejectCode::NotFound, "model profile not found");
        };
        revision_matches(profile.meta.revision, self.expected_revision)?;
        if self.name.as_deref().is_some_and(|n| !records::is_line(n)) {
            return reject(RejectCode::InvalidName, "name must be a single line");
        }
        if self.archived == Some(true) {
            let assigned: Vec<String> = view
                .agent_executions()?
                .into_iter()
                .filter(|cfg| cfg.model_profile_id == profile.id)
                .map(|cfg| cfg.employee_id.as_str().to_owned())
                .collect();
            if !assigned.is_empty() {
                return reject(
                    RejectCode::Conflict,
                    format!(
                        "PROFILE_IN_USE: assigned to employees: {}",
                        assigned.join(", ")
                    ),
                );
            }
            if view
                .watching_model()?
                .is_some_and(|w| w.model_profile_id.as_ref() == Some(&profile.id))
            {
                return reject(
                    RejectCode::Conflict,
                    "PROFILE_IN_USE: referenced by channel watching model",
                );
            }
        }
        if let Some(ref cids) = self.connection_ids {
            validate_pool_connections(view, cids)?;
            // Converting an auto-generated default into a manual pool clears its generated designation (`doc-89`).
            if profile.is_auto_generated && profile.connection_ids != *cids {
                profile.is_auto_generated = false;
                profile.generated_for_connection_id = None;
            }
            profile.connection_ids = cids.clone();
        }
        if let Some(ref name) = self.name {
            profile.name = name.clone();
        }
        if let Some(ref desc) = self.description {
            profile.description = desc.clone();
        }
        if let Some(archived) = self.archived {
            profile.archived = archived;
        }
        bump_meta(&mut profile.meta, &admitted.by, admitted.now);
        let reply = changed(&profile.id, profile.meta.revision, admitted.generation);
        Ok(Change::new(Op::UpdateModelProfile.ack())
            .put(
                records::model_profile_path(&profile.id),
                records::encode_model_profile(&profile),
            )
            .reply(reply))
    }
}

fn validate_pool_connections(view: &impl TeamDependents, cids: &[Id]) -> Result<(), Refusal> {
    if cids.is_empty() || cids.len() > 64 {
        return invalid("connection_ids must contain between 1 and 64 distinct connections");
    }
    let mut seen = std::collections::BTreeSet::new();
    for cid in cids {
        if !seen.insert(cid.as_str()) {
            return reject(
                RejectCode::InvalidRequest,
                "connection_ids must not contain duplicates",
            );
        }
        let Some(conn) = view.provider_connection(cid)? else {
            return reject(
                RejectCode::NotFound,
                format!("provider connection {} not found", cid.as_str()),
            );
        };
        if conn.archived {
            return reject(
                RejectCode::InvalidRequest,
                format!("provider connection {} is archived", cid.as_str()),
            );
        }
    }
    Ok(())
}
