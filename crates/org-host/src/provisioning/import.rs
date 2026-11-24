//! Opening an existing organization from Git repository files (`TASK-39` / R1, `doc-61` D25).
//!
//! Previews validate the repository's `state/` tree read-only via `agentic-org-office
//! --inspect-import` without touching `HostDb` or custody. Opening validates everything before
//! mutating `HostDb` or custody, acquires `self.gate` and `self.claim` cleanly (preventing
//! custody overwrite/deletion races and integrating with `stop_by()`), delegates HQ provisioning
//! to `Provisioning::drive` with `Pending.import_source` pinned to the validated Git commit OID,
//! binds the administrator to their chosen (or newly created) person record as owner, and returns
//! the canonical summary of local-only data that did not come back.

use std::sync::Arc;

use org_host_control::organization::{
    OpenExistingParams, OpenExistingResult, OpenPreviewPerson, OpenPreviewResult,
};
use org_office_launch::{
    ImportSourceV1, InspectImportPreview, InspectImportResponse, LOCAL_ONLY_NOT_RESTORED,
};
use org_proto::{
    selected_bootstrap::{
        valid_handle, valid_remote_url, CreatorV1, PlanV1, PreparedV1, HR_HANDLE,
    },
    ApiError, ErrorCode, RecoveryCode,
};

use super::{
    already_exists, generate_recovery_code, not_administrator, storage, unavailable, Pending,
    Provisioning, Reservation,
};
use crate::host_secrets::{Name, Value};
use crate::hostdb::accounts::Account;

/// Maximum characters allowed in a single-line person name (`org-office` `records::is_line`).
const MAX_LINE_CHARS: usize = 200;

impl Provisioning {
    /// `organization.open_preview`: validates an existing Git organization repository read-only
    /// and returns its preview metadata, active people list, and local-only loss summary.
    pub(crate) fn open_preview(
        &self,
        account: &Account,
        repository_remote_url: &str,
        state_branch: &str,
    ) -> Result<OpenPreviewResult, ApiError> {
        let preview = self.inspect_preview(account, repository_remote_url, state_branch)?;
        Ok(to_open_preview_result(preview))
    }

    fn inspect_preview(
        &self,
        account: &Account,
        repository_remote_url: &str,
        state_branch: &str,
    ) -> Result<InspectImportPreview, ApiError> {
        if !account.admin() || account.revoked_at_ms().is_some() {
            return Err(not_administrator());
        }
        let hq = self.hq.get().ok_or_else(unavailable)?;
        let preview = match hq.inspect_import(repository_remote_url, state_branch)? {
            InspectImportResponse::Ok { preview } => *preview,
            InspectImportResponse::Error { code, path, reason } => {
                return Err(map_inspect_error(&code, path, &reason));
            }
        };
        self.ensure_not_registered(&preview.organization_id, &preview.name, &preview.slug)?;
        Ok(preview)
    }

    /// `organization.open`: validates the Git repository, registers the organization under its
    /// existing UUIDv7 ID on this host with a new HQ office and new HQ CA, binds `account` to
    /// `params.owner_member_id` as owner, and starts serving the rebuilt projection immediately.
    pub(crate) fn open_existing(
        self: &Arc<Self>,
        account: &Account,
        params: &OpenExistingParams,
        fp: [u8; 32],
    ) -> Result<OpenExistingResult, ApiError> {
        if !account.admin() || account.revoked_at_ms().is_some() {
            return Err(not_administrator());
        }
        if self.stopping() {
            return Err(unavailable());
        }
        let hq = self.hq.get().ok_or_else(unavailable)?;
        validate_open_ids(params)?;

        let wall = now_wall_ms()?;
        let reservation = match self
            .db
            .provisioning(account, &params.operation_id, &fp, wall)
            .map_err(super::lookup_error)?
        {
            Some(existing) => existing,
            None => {
                // Validate every Git file and new_owner_person BEFORE touching HostDb or custody.
                let preview = self.inspect_preview(
                    account,
                    &params.repository_remote_url,
                    &params.state_branch,
                )?;
                let creator = resolve_owner_creator(
                    &preview,
                    &params.owner_member_id,
                    params.new_owner_person.as_ref(),
                )?;
                self.reserve_import(account, params, &preview, creator, fp, wall)?
            }
        };

        if !reservation.linked {
            let Some(_claim) = self.claim(&reservation.operation_id)? else {
                return Err(unavailable());
            };
            let fresh = self
                .db
                .provisioning_lookup(&reservation.account, &reservation.operation_id)
                .map_err(storage)?
                .ok_or_else(unavailable)?;
            if !fresh.linked {
                self.drive(&fresh, hq.as_ref())?;
            }
        }

        let organization = hq
            .organization(&reservation.prepared.organization_id)
            .ok_or_else(unavailable)?;
        let view = self.view(&reservation, organization, &reservation.owner_member)?;
        Ok(OpenExistingResult {
            organization: view,
            local_only_not_restored: LOCAL_ONLY_NOT_RESTORED
                .iter()
                .map(|s| (*s).to_owned())
                .collect(),
        })
    }

    fn ensure_not_registered(
        &self,
        organization_id: &str,
        name: &str,
        slug: &str,
    ) -> Result<(), ApiError> {
        if self
            .db
            .provisioning_for_org(organization_id)
            .map_err(storage)?
            .is_some()
        {
            let mut err = ApiError::new(
                ErrorCode::AlreadyExists,
                "this organization is already open on this instance",
            );
            err.field = Some("organization_id".into());
            err.rule = Some("taken".into());
            return Err(err);
        }
        if let Some(taken) = self.db.provisioning_taken(name, slug).map_err(storage)? {
            return Err(already_exists(taken));
        }
        Ok(())
    }

    fn reserve_import(
        &self,
        account: &Account,
        params: &OpenExistingParams,
        preview: &InspectImportPreview,
        creator: CreatorV1,
        fp: [u8; 32],
        wall: i64,
    ) -> Result<Reservation, ApiError> {
        let _gate = self.gate.lock().map_err(storage)?;
        if self.stopping() {
            return Err(unavailable());
        }
        // Re-check existing operation under the gate before touching custody so concurrent
        // requests can never overwrite or delete another request's recovery entry.
        if let Some(existing) = self
            .db
            .provisioning(account, &params.operation_id, &fp, wall)
            .map_err(super::lookup_error)?
        {
            return Ok(existing);
        }
        if self
            .db
            .provisioning_known(&params.operation_id)
            .map_err(storage)?
        {
            return Err(ApiError::new(
                ErrorCode::OperationReused,
                "operation_id already used",
            ));
        }
        self.ensure_not_registered(&preview.organization_id, &preview.name, &preview.slug)?;

        let plan = build_import_plan(params, preview)?;
        let prepared = PreparedV1 {
            plan,
            organization_id: preview.organization_id.clone(),
            office_id: crate::ids::uuid_v7().map_err(storage)?,
            repository_id: crate::ids::uuid_v7().map_err(storage)?,
            home_team_id: crate::ids::uuid_v7().map_err(storage)?,
            hr_employee_id: crate::ids::uuid_v7().map_err(storage)?,
            creator,
        };
        if !prepared.valid() {
            return Err(ApiError::new(
                ErrorCode::InvalidRequest,
                "imported organization metadata failed bootstrap validation",
            ));
        }

        let handle = Name::parse(&format!("recovery.{}", params.operation_id)).map_err(storage)?;
        let reservation = Reservation {
            operation_id: params.operation_id.clone(),
            account: account.account_id().into(),
            fingerprint: fp,
            prepared,
            owner_member: params.owner_member_id.clone(),
            recovery_handle: handle.as_str().into(),
            key_fingerprint: None,
            linked: false,
        };
        let code = generate_recovery_code()?;
        let pending = Pending {
            version: 1,
            reservation: reservation.clone(),
            code: RecoveryCode::new(code.expose().to_owned()).map_err(storage)?,
            import_source: Some(ImportSourceV1 {
                repository_remote_url: params.repository_remote_url.clone(),
                state_branch: params.state_branch.clone(),
                commit_oid: preview.commit_oid.clone(),
                new_owner_person: params.new_owner_person.clone(),
            }),
        };
        let bytes = Value::new(serde_json::to_vec(&pending).map_err(storage)?).map_err(storage)?;
        self.custody.put(&handle, &bytes).map_err(storage)?;
        let actual = self
            .custody
            .load(&handle)
            .map_err(storage)?
            .ok_or_else(unavailable)?;
        if actual.expose() != bytes.expose() {
            let _ = self.custody.delete(&handle);
            return Err(unavailable());
        }
        self.db
            .reserve_provisioning(account, &reservation)
            .map_err(|err| {
                // Never delete another committed operation's recovery entry on OperationReused.
                if !matches!(err, crate::hostdb::Error::OperationReused) {
                    let _ = self.custody.delete(&handle);
                }
                match err {
                    crate::hostdb::Error::NotAdministrator => not_administrator(),
                    crate::hostdb::Error::OperationReused => {
                        ApiError::new(ErrorCode::OperationReused, "operation_id already used")
                    }
                    crate::hostdb::Error::Conflict => ApiError::new(
                        ErrorCode::AlreadyExists,
                        "organization name, slug, or id already exists",
                    ),
                    _ => unavailable(),
                }
            })
    }
}

fn now_wall_ms() -> Result<i64, ApiError> {
    let wall = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(storage)?
        .as_millis();
    i64::try_from(wall).map_err(storage)
}

fn validate_open_ids(params: &OpenExistingParams) -> Result<(), ApiError> {
    if !org_proto::is_uuid_v7(&params.operation_id) {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "operation_id must be a canonical lowercase UUIDv7",
        ));
    }
    if !org_proto::is_uuid_v7(&params.owner_member_id) {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "owner_member_id must be a canonical lowercase UUIDv7",
        ));
    }
    Ok(())
}

fn build_import_plan(
    params: &OpenExistingParams,
    preview: &InspectImportPreview,
) -> Result<PlanV1, ApiError> {
    let plan_remote = if valid_remote_url(&params.repository_remote_url) {
        params.repository_remote_url.clone()
    } else if let Some(canonical) = preview
        .canonical_remote_url
        .as_deref()
        .filter(|u| valid_remote_url(u))
    {
        canonical.to_owned()
    } else {
        "https://localhost/imported.git".into()
    };
    let plan = PlanV1 {
        version: 1,
        name: preview.name.clone(),
        slug: preview.slug.clone(),
        repository_remote_url: plan_remote,
        state_branch: params.state_branch.clone(),
    };
    if let Some((field, rule)) = plan.invalid_field() {
        let mut err = ApiError::new(
            ErrorCode::InvalidRequest,
            format!("invalid imported organization field: {field}"),
        );
        err.field = Some(field.into());
        err.rule = Some(rule.into());
        return Err(err);
    }
    Ok(plan)
}

fn to_open_preview_result(preview: InspectImportPreview) -> OpenPreviewResult {
    OpenPreviewResult {
        organization_id: preview.organization_id,
        name: preview.name,
        slug: preview.slug,
        repository_remote_url: preview.repository_remote_url,
        canonical_remote_url: preview.canonical_remote_url,
        state_branch: preview.state_branch,
        teams_count: preview.teams_count,
        projects_count: preview.projects_count,
        agents_count: preview.agents_count,
        people: preview
            .people
            .into_iter()
            .map(|p| OpenPreviewPerson {
                id: p.id,
                name: p.name,
                handle: p.handle,
                home_team_id: p.home_team_id,
            })
            .collect(),
        local_only_not_restored: preview.local_only_not_restored,
    }
}

fn is_valid_line(text: &str) -> bool {
    !text.is_empty()
        && text.trim() == text
        && text.chars().count() <= MAX_LINE_CHARS
        && !text.chars().any(char::is_control)
}

fn resolve_owner_creator(
    preview: &InspectImportPreview,
    owner_member_id: &str,
    new_owner_person: Option<&CreatorV1>,
) -> Result<CreatorV1, ApiError> {
    if let Some(existing) = preview.people.iter().find(|p| p.id == owner_member_id) {
        // Existing records keep their exact Git employee record; PreparedV1.creator only
        // requires a bootstrap-valid handle/name shape for HostDb reservation invariants.
        if is_valid_line(&existing.name)
            && valid_handle(&existing.handle)
            && existing.handle != HR_HANDLE
        {
            return Ok(CreatorV1 {
                employee_id: existing.id.clone(),
                name: existing.name.clone(),
                handle: existing.handle.clone(),
            });
        }
        return Ok(CreatorV1::from_login(existing.id.clone(), &existing.handle));
    }
    let Some(new_person) = new_owner_person else {
        let mut err = ApiError::new(
            ErrorCode::InvalidRequest,
            "owner_member_id must name an active person in the repository or include new_owner_person",
        );
        err.field = Some("owner_member_id".into());
        err.rule = Some("not_found".into());
        return Err(err);
    };
    validate_new_owner_person(preview, owner_member_id, new_person)?;
    Ok(new_person.clone())
}

fn validate_new_owner_person(
    preview: &InspectImportPreview,
    owner_member_id: &str,
    new_person: &CreatorV1,
) -> Result<(), ApiError> {
    if new_person.employee_id != owner_member_id {
        return Err(field_err(
            "owner_member_id",
            "mismatch",
            "new_owner_person.employee_id must equal owner_member_id",
        ));
    }
    if !is_valid_line(&new_person.name) {
        return Err(field_err(
            "new_owner_person.name",
            "format",
            "new_owner_person.name must be a trimmed single line of at most 200 characters",
        ));
    }
    if !valid_handle(&new_person.handle) || new_person.handle == HR_HANDLE {
        return Err(field_err(
            "new_owner_person.handle",
            "format",
            "new_owner_person.handle must be a valid non-reserved handle",
        ));
    }
    if preview
        .reserved_handles
        .iter()
        .any(|h| h == &new_person.handle)
    {
        return Err(field_err(
            "new_owner_person.handle",
            "taken",
            "new_owner_person.handle is already used by an employee in this organization",
        ));
    }
    if preview
        .reserved_entity_ids
        .iter()
        .any(|id| id == &new_person.employee_id)
    {
        return Err(field_err(
            "owner_member_id",
            "taken",
            "owner_member_id collides with an existing entity in this organization",
        ));
    }
    if !preview.has_active_root_team {
        return Err(field_err(
            "new_owner_person",
            "unavailable",
            "cannot add a new owner person because the repository has no active root team",
        ));
    }
    Ok(())
}

fn field_err(field: &str, rule: &str, message: &str) -> ApiError {
    let mut err = ApiError::new(ErrorCode::InvalidRequest, message);
    err.field = Some(field.into());
    err.rule = Some(rule.into());
    err
}

fn map_inspect_error(code: &str, path: Option<String>, reason: &str) -> ApiError {
    match code {
        "invalid_file" => {
            let file = path.unwrap_or_else(|| "state/organization.yaml".into());
            let mut err = ApiError::new(ErrorCode::InvalidRequest, format!("{file}: {reason}"));
            err.field = Some(file);
            err.rule = Some("invalid_file".into());
            err
        }
        "git_failed" => {
            let mut err = ApiError::new(
                ErrorCode::InvalidRequest,
                format!("could not read Git repository: {reason}"),
            );
            err.field = Some("repository_remote_url".into());
            err.rule = Some("git_failed".into());
            err
        }
        "invalid_request" => ApiError::new(ErrorCode::InvalidRequest, reason),
        _ => ApiError::new(ErrorCode::StorageError, reason),
    }
}
