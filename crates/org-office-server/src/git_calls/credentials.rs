//! Repository origins and office Git credentials: HTTPS/SSH origin validation (authority, host and
//! port), local-origin recording, credential host directories, and remote spec resolution.

use std::path::Path;

use org_git::{
    validate_local_origin_path, validate_remote_url, AllowedProtocol, GitCredential, RemoteSpec,
};
use org_office::organization_family::records::{Repository, RepositoryKind};
use org_office::Office;
use org_proto::git::{RepoCredentialView, RepoRegisterParams, RepositoryKindWire};
use org_proto::{ApiError, ErrorCode};

use super::redaction::{map_git_error, map_office_error};
use super::vault;

pub(super) fn validate_register_origin(
    params: &RepoRegisterParams,
) -> Result<Option<String>, ApiError> {
    match params.kind {
        RepositoryKindWire::Remote => {
            if params.origin_path.is_some() {
                return Err(ApiError::new(
                    ErrorCode::InvalidRequest,
                    "remote repository cannot specify origin_path",
                ));
            }
            let url = params.remote_url.as_deref().ok_or_else(|| {
                ApiError::new(
                    ErrorCode::InvalidRequest,
                    "remote repository requires remote_url",
                )
            })?;
            validate_remote_url(url).map_err(map_git_error)?;
            Ok(None)
        }
        RepositoryKindWire::Local => {
            if params.remote_url.is_some() {
                return Err(ApiError::new(
                    ErrorCode::InvalidRequest,
                    "local repository cannot specify remote_url",
                ));
            }
            let origin = params.origin_path.as_deref().ok_or_else(|| {
                ApiError::new(
                    ErrorCode::InvalidRequest,
                    "local repository requires origin_path",
                )
            })?;
            let canon = validate_local_origin_path(Path::new(origin)).map_err(map_git_error)?;
            Ok(Some(canon.to_string_lossy().into_owned()))
        }
    }
}

pub(super) fn record_or_verify_local_origin(
    office: &Office,
    repo: &Repository,
    origin_path: &str,
) -> Result<(), ApiError> {
    if let Some(existing) = office
        .read_local_repo_origin(&repo.id)
        .map_err(map_office_error)?
    {
        if existing != origin_path {
            return Err(ApiError::new(
                ErrorCode::OperationReused,
                "command_id reused with different local origin_path",
            ));
        }
    } else {
        office
            .write_local_repo_origin(&repo.id, origin_path)
            .map_err(map_office_error)?;
    }
    Ok(())
}

pub(super) fn resolve_remote_spec(
    office: &Office,
    repo: &Repository,
) -> Result<RemoteSpec, ApiError> {
    match repo.kind {
        RepositoryKind::Remote => {
            let url = repo.remote_url.as_deref().ok_or_else(|| {
                ApiError::new(
                    ErrorCode::InvalidRequest,
                    "remote repository has no remote_url",
                )
            })?;
            let protocol = validate_remote_url(url).map_err(map_git_error)?;
            let credential = resolve_office_git_credential_for_repo(
                office,
                Some(repo.id.as_str()),
                url,
                protocol,
            )?;
            Ok(RemoteSpec {
                url: url.to_owned(),
                protocol,
                credential,
            })
        }
        RepositoryKind::Local => {
            let origin = office
                .read_local_repo_origin(&repo.id)
                .map_err(map_office_error)?
                .ok_or_else(|| {
                    ApiError::new(
                        ErrorCode::NotFound,
                        "local repository origin_path is not configured in this office",
                    )
                })?;
            let canon = validate_local_origin_path(Path::new(&origin)).map_err(map_git_error)?;
            Ok(RemoteSpec {
                url: canon.to_string_lossy().into_owned(),
                protocol: AllowedProtocol::File,
                credential: GitCredential::None,
            })
        }
    }
}

fn is_valid_https_host(host: &str) -> bool {
    if host.is_empty() {
        return false;
    }
    if let Some(inner) = host.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
        return inner.parse::<std::net::Ipv6Addr>().is_ok();
    }
    host.bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.'))
        && !host.starts_with('.')
        && !host.ends_with('.')
        && !host.contains("..")
}

fn split_https_host_port(authority: &str) -> Result<(&str, Option<&str>), ApiError> {
    if authority.starts_with('[') {
        let end = authority.find(']').ok_or_else(|| {
            ApiError::new(
                ErrorCode::InvalidRequest,
                "HTTPS remote URL has malformed IPv6 host",
            )
        })?;
        let host = &authority[..=end];
        let tail = &authority[end + 1..];
        if tail.is_empty() {
            Ok((host, None))
        } else if let Some(port) = tail.strip_prefix(':') {
            Ok((host, Some(port)))
        } else {
            Err(ApiError::new(
                ErrorCode::InvalidRequest,
                "HTTPS remote URL has invalid IPv6 port suffix",
            ))
        }
    } else if let Some((h, p)) = authority.rsplit_once(':') {
        Ok((h, Some(p)))
    } else {
        Ok((authority, None))
    }
}

/// Extracts a strictly validated `https://<host>[:<port>]` origin scope from an HTTPS remote URL
/// so `git -c credential.<scope>.helper=...` can never be split (`=`) or wildcarded (`*`) across
/// unrelated hosts (`TASK-161`, `doc-35`).
pub(super) fn https_origin_scope(url: &str) -> Result<String, ApiError> {
    let rest = url.trim().strip_prefix("https://").ok_or_else(|| {
        ApiError::new(
            ErrorCode::InvalidRequest,
            "HTTPS remote URL must start with https://",
        )
    })?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let bad_char = |b: u8| {
        b.is_ascii_control()
            || b.is_ascii_whitespace()
            || matches!(b, b'@' | b'=' | b'*' | b'\\' | b'"' | b'\'' | b';')
    };
    if authority.is_empty() || authority.bytes().any(bad_char) {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "HTTPS remote URL has invalid authority",
        ));
    }
    let (host, port_opt) = split_https_host_port(authority)?;
    if !is_valid_https_host(host) {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "HTTPS remote URL has invalid host",
        ));
    }
    if let Some(port_str) = port_opt {
        let port: u16 = port_str.parse().map_err(|_| {
            ApiError::new(
                ErrorCode::InvalidRequest,
                "HTTPS remote URL has invalid port",
            )
        })?;
        if port == 0 || !port_str.bytes().all(|b| b.is_ascii_digit()) {
            return Err(ApiError::new(
                ErrorCode::InvalidRequest,
                "HTTPS remote URL has invalid port",
            ));
        }
    }
    Ok(format!("https://{authority}"))
}

fn validate_credential_host_dir(host: &str) -> Result<String, ApiError> {
    let lower = host.trim().to_ascii_lowercase();
    let stripped = lower
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .unwrap_or(&lower);
    let safe_ipv6 = stripped.contains(':') && stripped.parse::<std::net::Ipv6Addr>().is_ok();
    let safe_dns = !lower.is_empty()
        && !lower.starts_with('.')
        && !lower.starts_with('-')
        && !lower.ends_with('.')
        && !lower.contains("..")
        && lower
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.'));
    if !safe_dns && !safe_ipv6 {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "remote URL has an invalid host for credential lookup",
        ));
    }
    Ok(if safe_ipv6 {
        stripped.replace(':', "_")
    } else {
        lower
    })
}

/// Extracts a strictly validated lowercase host directory name from an HTTPS or SSH remote URL
/// so per-host credentials never bleed across unrelated hosts (`TASK-168`, `doc-35`).
pub(super) fn extract_remote_host(
    url: &str,
    protocol: AllowedProtocol,
) -> Result<String, ApiError> {
    let trimmed = url.trim();
    match protocol {
        AllowedProtocol::Https => {
            let scope = https_origin_scope(trimmed)?;
            let authority = scope.strip_prefix("https://").unwrap_or("");
            let (host, _) = split_https_host_port(authority)?;
            validate_credential_host_dir(host)
        }
        AllowedProtocol::Ssh => {
            if trimmed.contains('?') || trimmed.contains('#') {
                return Err(ApiError::new(
                    ErrorCode::InvalidRequest,
                    "SSH remote URL must not contain query or fragment",
                ));
            }
            let host_part = if let Some(rest) = trimmed.strip_prefix("ssh://") {
                let authority = rest.split('/').next().unwrap_or("");
                if authority.matches('@').count() > 1 {
                    return Err(ApiError::new(
                        ErrorCode::InvalidRequest,
                        "SSH remote URL has an ambiguous userinfo authority",
                    ));
                }
                let after_user = match authority.split_once('@') {
                    Some((user, host_port)) => {
                        if user.is_empty() || user.contains(':') {
                            return Err(ApiError::new(
                                ErrorCode::InvalidRequest,
                                "SSH remote URL has an invalid userinfo component",
                            ));
                        }
                        host_port
                    }
                    None => authority,
                };
                let (host, port) = split_https_host_port(after_user)?;
                if let Some(p) = port {
                    if !p.bytes().all(|b| b.is_ascii_digit())
                        || p.parse::<u16>().ok().filter(|n| *n > 0).is_none()
                    {
                        return Err(ApiError::new(
                            ErrorCode::InvalidRequest,
                            "SSH remote URL has an invalid port",
                        ));
                    }
                }
                host
            } else if let Some((user_host, _)) = trimmed.split_once(':') {
                if user_host.matches('@').count() > 1 {
                    return Err(ApiError::new(
                        ErrorCode::InvalidRequest,
                        "SSH remote URL has an ambiguous userinfo authority",
                    ));
                }
                user_host.split_once('@').map_or(user_host, |(_, h)| h)
            } else {
                ""
            };
            validate_credential_host_dir(host_part)
        }
        AllowedProtocol::File | AllowedProtocol::None => Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "local or protocol-less repositories do not use remote credentials",
        )),
    }
}

fn check_optional_cred_entry(path: &Path, label: &str) -> Result<bool, ApiError> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err(ApiError::new(
            ErrorCode::StorageError,
            format!("could not inspect office {label}"),
        )),
    }
}

/// Resolves an office-local Git remote credential (`TASK-135`, `TASK-168`, `doc-35`).
///
/// For HTTPS remotes, prefers an encrypted per-repository vault token at
/// `<office.folder()>/git-credentials/repos/<repository_id>/credential.v1.json` bound to the
/// repository URL, then falls back to an owner-only per-host helper at
/// `<office.folder()>/git-credentials/hosts/<host>/https-helper`. Never reads personal host `gh`,
/// `~/.git-credentials`, `~/.gitconfig`, or ambient `GH_TOKEN`/`GITHUB_TOKEN`.
pub(super) fn resolve_office_git_credential_for_repo(
    office: &Office,
    repo_id: Option<&str>,
    url: &str,
    protocol: AllowedProtocol,
) -> Result<GitCredential, ApiError> {
    if matches!(protocol, AllowedProtocol::File | AllowedProtocol::None) {
        return Ok(GitCredential::None);
    }
    if protocol == AllowedProtocol::Https {
        if let Some(rid) = repo_id {
            if let Some(cred) = vault::load_repo_https_token(office, rid, url)? {
                return Ok(cred);
            }
        }
    }
    resolve_host_scoped_git_credential(office, url, protocol)
}

/// Resolves a host-scoped Git credential (existing TASK-168 helper/SSH API kept for tests).
#[cfg(test)]
pub(super) fn resolve_office_git_credential(
    office: &Office,
    url: &str,
    protocol: AllowedProtocol,
) -> Result<GitCredential, ApiError> {
    resolve_office_git_credential_for_repo(office, None, url, protocol)
}

/// Inspects non-secret credential status for `repo` on this office without returning any secret.
pub(super) fn inspect_repo_credential_status(
    office: &Office,
    repo: &Repository,
) -> Result<RepoCredentialView, ApiError> {
    if let Some(view) = vault::vault_credential_status(office, repo)? {
        return Ok(view);
    }
    let office_id = office.info().office_id.clone();
    if repo.kind != RepositoryKind::Remote {
        return Ok(unconfigured_credential_view(office_id, repo, None));
    }
    let Some(url) = repo.remote_url.as_deref() else {
        return Ok(unconfigured_credential_view(office_id, repo, None));
    };
    let protocol = validate_remote_url(url).map_err(map_git_error)?;
    let scope_url = (protocol == AllowedProtocol::Https)
        .then(|| vault::canonical_https_repo_scope(url).ok())
        .flatten();
    match resolve_host_scoped_git_credential(office, url, protocol)? {
        GitCredential::HttpsHelper { scope_url, .. } => Ok(RepoCredentialView {
            office_id,
            repository_id: repo.id.as_str().to_owned(),
            repository_key: repo.key.clone(),
            configured: true,
            source: Some("host_helper".into()),
            scope_url: Some(scope_url),
            username: None,
            updated_at: None,
        }),
        GitCredential::Ssh { .. } => Ok(RepoCredentialView {
            office_id,
            repository_id: repo.id.as_str().to_owned(),
            repository_key: repo.key.clone(),
            configured: true,
            source: Some("host_ssh".into()),
            scope_url: None,
            username: None,
            updated_at: None,
        }),
        GitCredential::None | GitCredential::HttpsToken { .. } => {
            Ok(unconfigured_credential_view(office_id, repo, scope_url))
        }
    }
}

fn unconfigured_credential_view(
    office_id: String,
    repo: &Repository,
    scope_url: Option<String>,
) -> RepoCredentialView {
    RepoCredentialView {
        office_id,
        repository_id: repo.id.as_str().to_owned(),
        repository_key: repo.key.clone(),
        configured: false,
        source: None,
        scope_url,
        username: None,
        updated_at: None,
    }
}

fn resolve_host_scoped_git_credential(
    office: &Office,
    url: &str,
    protocol: AllowedProtocol,
) -> Result<GitCredential, ApiError> {
    if matches!(protocol, AllowedProtocol::File | AllowedProtocol::None) {
        return Ok(GitCredential::None);
    }
    let host_key = extract_remote_host(url, protocol)?;
    let creds_root = office.folder().join("git-credentials");
    let hosts_dir = creds_root.join("hosts");
    let host_dir = hosts_dir.join(&host_key);
    for (dir, label) in [
        (&creds_root, "git-credentials directory"),
        (&hosts_dir, "git-credentials hosts directory"),
        (&host_dir, "git-credentials host directory"),
    ] {
        match std::fs::symlink_metadata(dir) {
            Ok(meta) if meta.file_type().is_dir() => {}
            Ok(_) => {
                return Err(ApiError::new(
                    ErrorCode::StorageError,
                    format!("office {label} must be a real directory (never a symlink)"),
                ));
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return Ok(GitCredential::None);
            }
            Err(_) => {
                return Err(ApiError::new(
                    ErrorCode::StorageError,
                    format!("could not inspect office {label}"),
                ));
            }
        }
    }
    let expected_uid = org_platform::fs::effective_uid().map_err(|_| {
        ApiError::new(
            ErrorCode::StorageError,
            "office git credentials are unsupported on this platform",
        )
    })?;
    org_platform::fs::verify_owner_only_ancestors(&host_dir, office.folder(), expected_uid)
        .map_err(|_| {
            ApiError::new(
                ErrorCode::StorageError,
                "office git-credentials directory hierarchy failed owner-only verification",
            )
        })?;

    match protocol {
        AllowedProtocol::Https => {
            let helper = host_dir.join("https-helper");
            if !check_optional_cred_entry(&helper, "https-helper")? {
                return Ok(GitCredential::None);
            }
            let helper_program =
                org_platform::fs::verify_owner_only_file(&helper, expected_uid, true).map_err(
                    |_| {
                        ApiError::new(
                            ErrorCode::StorageError,
                            "office git https-helper failed owner-only executable verification",
                        )
                    },
                )?;
            let scope_url = https_origin_scope(url)?;
            Ok(GitCredential::HttpsHelper {
                helper_program,
                scope_url,
            })
        }
        AllowedProtocol::Ssh => {
            let key = host_dir.join("ssh-key");
            if !check_optional_cred_entry(&key, "ssh-key")? {
                return Ok(GitCredential::None);
            }
            let identity_file = org_platform::fs::verify_owner_only_file(&key, expected_uid, false)
                .map_err(|_| {
                    ApiError::new(
                        ErrorCode::StorageError,
                        "office git ssh-key failed owner-only verification",
                    )
                })?;
            let known_hosts = host_dir.join("known-hosts");
            let known_hosts_file =
                org_platform::fs::verify_owner_only_file(&known_hosts, expected_uid, false)
                    .map_err(|_| {
                        ApiError::new(
                            ErrorCode::StorageError,
                            "office git known-hosts failed owner-only verification",
                        )
                    })?;
            Ok(GitCredential::Ssh {
                identity_file,
                known_hosts_file,
            })
        }
        AllowedProtocol::File | AllowedProtocol::None => Ok(GitCredential::None),
    }
}
