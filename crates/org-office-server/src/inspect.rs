//! `--inspect-import`: validates an organization's Git state branch read-only and answers one JSON
//! line to the host.

use crate::fdswap::ControlPipe;
use org_office_launch::{
    InspectImportPerson, InspectImportPreview, InspectImportRequest, InspectImportResponse,
};

/// `--inspect-import`: reads one JSON request line from `control.from_host`, clones and
/// validates the organization's Git state branch read-only via `org_office::import`, and writes
/// one JSON [`InspectImportResponse`] line to `control.to_host`.
pub(crate) fn inspect_import(mut control: ControlPipe) -> i32 {
    org_office_sqlite::install();
    use std::io::{BufRead, BufReader, Read, Write};
    let mut line = String::new();
    let read_res = BufReader::new(&mut control.from_host)
        .take(16 * 1024)
        .read_line(&mut line);
    let response = match read_res
        .ok()
        .and_then(|n| (n > 0).then(|| serde_json::from_str::<InspectImportRequest>(&line).ok()))
        .flatten()
    {
        None => InspectImportResponse::Error {
            code: "invalid_request".into(),
            path: None,
            reason: "invalid inspect-import request".into(),
        },
        Some(req) => match org_office::import::clone_and_validate_state(
            &req.repository_remote_url,
            &req.state_branch,
        ) {
            Ok(imported) => InspectImportResponse::Ok {
                preview: Box::new(InspectImportPreview {
                    organization_id: imported.summary.organization_id.as_str().to_owned(),
                    name: imported.summary.name,
                    slug: imported.summary.slug,
                    repository_remote_url: imported.repository_remote_url,
                    canonical_remote_url: imported.summary.canonical_remote_url,
                    state_branch: imported.state_branch,
                    commit_oid: imported.commit_oid,
                    teams_count: imported.summary.teams_count,
                    projects_count: imported.summary.projects_count,
                    agents_count: imported.summary.agents_count,
                    people: imported
                        .summary
                        .people
                        .into_iter()
                        .map(|p| InspectImportPerson {
                            id: p.id,
                            name: p.name,
                            handle: p.handle,
                            home_team_id: p.home_team_id,
                        })
                        .collect(),
                    reserved_handles: imported.summary.reserved_handles,
                    reserved_entity_ids: imported.summary.reserved_entity_ids,
                    has_active_root_team: imported.summary.has_active_root_team,
                    local_only_not_restored: imported.local_only_not_restored,
                }),
            },
            Err(org_office::import::ImportError::InvalidFile { path, reason }) => {
                InspectImportResponse::Error {
                    code: "invalid_file".into(),
                    path: Some(path),
                    reason,
                }
            }
            Err(org_office::import::ImportError::GitFailed(reason)) => {
                InspectImportResponse::Error {
                    code: "git_failed".into(),
                    path: None,
                    reason,
                }
            }
            Err(org_office::import::ImportError::InvalidInput(reason)) => {
                InspectImportResponse::Error {
                    code: "invalid_request".into(),
                    path: None,
                    reason,
                }
            }
            Err(org_office::import::ImportError::Storage(reason)) => InspectImportResponse::Error {
                code: "storage_error".into(),
                path: None,
                reason,
            },
        },
    };
    if let Ok(json) = serde_json::to_string(&response) {
        let _ = writeln!(control.to_host, "{json}");
        let _ = control.to_host.flush();
    }
    0
}
