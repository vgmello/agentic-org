//! Unit and integration tests for encrypted per-repo office Git vault credentials (`TASK-135`).

use std::os::unix::fs::PermissionsExt as _;
use std::sync::Arc;

use org_git::{AllowedProtocol, GitCredential, GitError};
use org_office::{EngineChoice, Office, TestOffice};
use org_proto::git::{
    RepoCredentialRevokeParams, RepoCredentialSetParams, RepoCredentialStatusParams,
    RepoRegisterParams, RepositoryKindWire,
};
use org_proto::org::{MemberAddPersonParams, TeamCreateParams};
use org_proto::ErrorCode;

use crate::live::LiveEvents;

use super::credentials::{inspect_repo_credential_status, resolve_office_git_credential_for_repo};
use super::redaction::{map_remote_git_error, sanitized_remote_git_reason, scrub_git_log_text};
use super::*;

fn uuid(n: u64) -> String {
    format!("018bce83-1350-7000-8000-{n:012x}")
}

fn register_https_repo(svc: &GitService, owner: &str, id: &str, key: &str, url: &str) {
    svc.handle_register(
        owner,
        RepoRegisterParams {
            command_id: uuid(900 + key.len() as u64 + id.as_bytes()[35] as u64),
            repository_id: id.to_owned(),
            key: Some(key.to_owned()),
            kind: RepositoryKindWire::Remote,
            remote_url: Some(url.to_owned()),
            origin_path: None,
        },
    )
    .expect("register https repo");
}

#[test]
fn task135_per_repo_vault_credential_lifecycle_isolation_and_fail_closed_security() {
    org_office_sqlite::install();
    let office = Arc::new(Office::for_test(TestOffice {
        engine: EngineChoice::Sqlite,
    }));
    org_platform::fs::set_owner_only_dir(office.folder()).unwrap();
    let owner = office.owner_member_id().to_owned();
    let live = Arc::new(LiveEvents::for_owner(Box::new(|| {}), owner.clone()));
    let svc = GitService::new(Arc::clone(&office), live);

    // Add a non-owner member to verify owner-only authorization.
    let root_team = office.org_snapshot().unwrap().teams[0].id.clone();
    let team_id = uuid(10);
    let non_owner = uuid(11);
    let _ = office
        .submit_org(
            &owner,
            org_office::rules::OrgCommand::TeamCreate(TeamCreateParams {
                command_id: uuid(12),
                team_id: team_id.clone(),
                name: "Workers".into(),
                owner_team_id: Some(root_team),
            }),
        )
        .unwrap()
        .unwrap();
    let _ = office
        .submit_org(
            &owner,
            org_office::rules::OrgCommand::MemberAddPerson(MemberAddPersonParams {
                command_id: uuid(13),
                member_id: non_owner.clone(),
                name: "Worker One".into(),
                handle: Some("worker1".into()),
                home_team_id: Some(team_id),
                contact: None,
            }),
        )
        .unwrap()
        .unwrap();

    let repo_a_id = uuid(101);
    let repo_b_id = uuid(102);
    let repo_a_url = "https://github.com/acme/repo-a.git";
    let repo_b_url = "https://github.com/acme/repo-b.git";
    register_https_repo(&svc, &owner, &repo_a_id, "repo-a", repo_a_url);
    register_https_repo(&svc, &owner, &repo_b_id, "repo-b", repo_b_url);

    // Non-owner is refused for set, status, and revoke.
    assert_eq!(
        svc.handle_credential_set(
            &non_owner,
            RepoCredentialSetParams {
                repository: "repo-a".into(),
                username: None,
                token: "ghp_denied".into(),
            },
        )
        .unwrap_err()
        .code,
        ErrorCode::NotAuthorized
    );
    assert_eq!(
        svc.handle_credential_status(
            &non_owner,
            RepoCredentialStatusParams {
                repository: "repo-a".into(),
            },
        )
        .unwrap_err()
        .code,
        ErrorCode::NotAuthorized
    );
    assert_eq!(
        svc.handle_credential_revoke(
            &non_owner,
            RepoCredentialRevokeParams {
                repository: "repo-a".into(),
            },
        )
        .unwrap_err()
        .code,
        ErrorCode::NotAuthorized
    );

    // Initially neither repo has credentials configured.
    let before = svc
        .handle_credential_status(
            &owner,
            RepoCredentialStatusParams {
                repository: "repo-a".into(),
            },
        )
        .unwrap();
    assert!(!before.configured);
    assert_eq!(before.source, None);

    // Store an encrypted per-repo HTTPS token for `repo-a`.
    let secret = "ghp_TASK135_VAULT_SECRET_REPO_A_999";
    let stored = svc
        .handle_credential_set(
            &owner,
            RepoCredentialSetParams {
                repository: "repo-a".into(),
                username: Some("x-access-token".into()),
                token: secret.into(),
            },
        )
        .expect("store per-repo vault credential");
    assert!(stored.configured);
    assert_eq!(stored.source.as_deref(), Some("vault"));
    assert_eq!(stored.scope_url.as_deref(), Some(repo_a_url));
    assert_eq!(stored.username.as_deref(), Some("x-access-token"));
    assert!(stored.updated_at.is_some());

    // Verify encrypted vault file sits outside durable/ & runtime/, has 0700/0600 modes, and
    // never stores the plaintext token on disk.
    let vault_file = office
        .folder()
        .join("git-credentials")
        .join("repos")
        .join(&repo_a_id)
        .join("credential.v1.json");
    assert!(!vault_file.starts_with(office.durable_root()));
    assert!(!vault_file.starts_with(office.runtime_root()));
    assert_eq!(
        std::fs::metadata(&vault_file).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(
        std::fs::metadata(vault_file.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    let raw_disk = std::fs::read_to_string(&vault_file).unwrap();
    assert!(
        !raw_disk.contains(secret),
        "vault file on disk must never contain plaintext secret"
    );

    // Resolution attaches the token ONLY to `repo-a`; sibling `repo-b` on the same github.com
    // host gets `GitCredential::None` (strict per-repo isolation!).
    match resolve_office_git_credential_for_repo(
        &office,
        Some(&repo_a_id),
        repo_a_url,
        AllowedProtocol::Https,
    )
    .unwrap()
    {
        GitCredential::HttpsToken {
            username,
            token,
            scope_url,
        } => {
            assert_eq!(username, "x-access-token");
            assert_eq!(token.as_str(), secret);
            assert_eq!(scope_url, repo_a_url);
        }
        other => panic!("expected HttpsToken for repo-a, got {other:?}"),
    }
    assert!(
        matches!(
            resolve_office_git_credential_for_repo(
                &office,
                Some(&repo_b_id),
                repo_b_url,
                AllowedProtocol::Https,
            )
            .unwrap(),
            GitCredential::None
        ),
        "repo-a vault token must NEVER be returned for repo-b on the same host"
    );

    // Copying repo-a's encrypted envelope into repo-b's directory fails closed due to AAD binding!
    let repo_b_file = office
        .folder()
        .join("git-credentials")
        .join("repos")
        .join(&repo_b_id)
        .join("credential.v1.json");
    org_platform::fs::create_owner_only_dir_all(repo_b_file.parent().unwrap()).unwrap();
    org_platform::fs::write_owner_only_file(&repo_b_file, raw_disk.as_bytes()).unwrap();
    assert!(
        resolve_office_git_credential_for_repo(
            &office,
            Some(&repo_b_id),
            repo_b_url,
            AllowedProtocol::Https,
        )
        .is_err(),
        "swapped encrypted envelope must fail AAD verification"
    );
    std::fs::remove_file(&repo_b_file).unwrap();

    // Widened file permissions (0644) fail closed.
    std::fs::set_permissions(&vault_file, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(resolve_office_git_credential_for_repo(
        &office,
        Some(&repo_a_id),
        repo_a_url,
        AllowedProtocol::Https,
    )
    .is_err());
    std::fs::set_permissions(&vault_file, std::fs::Permissions::from_mode(0o600)).unwrap();

    // Host-scoped helper still works as fallback for repo-b, while repo-a prefers its vault token.
    let gh_dir = office
        .folder()
        .join("git-credentials")
        .join("hosts")
        .join("github.com");
    org_platform::fs::create_owner_only_dir_all(&gh_dir).unwrap();
    let helper = gh_dir.join("https-helper");
    std::fs::write(&helper, "#!/bin/sh\nexit 0\n").unwrap();
    std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert!(matches!(
        resolve_office_git_credential_for_repo(
            &office,
            Some(&repo_a_id),
            repo_a_url,
            AllowedProtocol::Https,
        )
        .unwrap(),
        GitCredential::HttpsToken { .. }
    ));
    assert!(matches!(
        resolve_office_git_credential_for_repo(
            &office,
            Some(&repo_b_id),
            repo_b_url,
            AllowedProtocol::Https,
        )
        .unwrap(),
        GitCredential::HttpsHelper { .. }
    ));
    let repo_b_record = office.resolve_repository(&repo_b_id).unwrap().unwrap();
    let b_status = inspect_repo_credential_status(&office, &repo_b_record).unwrap();
    assert!(b_status.configured);
    assert_eq!(b_status.source.as_deref(), Some("host_helper"));

    // Revoking repo-a's vault token removes the file and reports clean status.
    std::fs::remove_file(&helper).unwrap();
    let revoked = svc
        .handle_credential_revoke(
            &owner,
            RepoCredentialRevokeParams {
                repository: "repo-a".into(),
            },
        )
        .expect("revoke vault token");
    assert!(!revoked.configured);
    assert_eq!(revoked.source, None);
    assert!(!vault_file.exists());
}

#[test]
fn task135_actionable_https_auth_error_and_secret_redaction() {
    use org_git::RemoteSpec;
    use zeroize::Zeroizing;

    let secret_owned = ["test", "token", "stderr", "redaction"].join("-");
    let basic_owned = ["dXNlcjp0ZXN0", "dG9rZW4="].concat();
    let (secret, basic) = (secret_owned.as_str(), basic_owned.as_str());
    let stderr = format!(
        "fatal: could not read Username for 'https://x-access-token:{secret}@github.com/acme/private.git': terminal prompts disabled\nGIT_CONFIG_VALUE_0=Authorization: Basic {basic}"
    );
    let remote_unauth = RemoteSpec {
        url: "https://github.com/acme/private.git".into(),
        protocol: AllowedProtocol::Https,
        credential: GitCredential::None,
    };
    let remote_vault = RemoteSpec {
        url: "https://github.com/acme/private.git".into(),
        protocol: AllowedProtocol::Https,
        credential: GitCredential::HttpsToken {
            username: "x-access-token".into(),
            token: Zeroizing::new(secret.into()),
            scope_url: "https://github.com/acme/private.git".into(),
        },
    };

    // Actionable fail-closed operator hint when no vault token is configured.
    let api_missing = map_remote_git_error(
        GitError::RemoteUnavailable(stderr.clone()),
        &remote_unauth,
        "private-app",
    );
    assert_eq!(api_missing.code, ErrorCode::NotAuthorized);
    assert!(
        api_missing
            .message
            .contains("agentic-org repo set-credential --repo private-app"),
        "missing actionable hint: {}",
        api_missing.message
    );
    assert!(!api_missing.message.contains(secret));
    assert!(!api_missing.message.contains(basic));

    // Actionable rotation hint when a vault token IS configured but rejected by remote.
    let api_rejected = map_remote_git_error(
        GitError::RemoteUnavailable(stderr.clone()),
        &remote_vault,
        "private-app",
    );
    assert_eq!(api_rejected.code, ErrorCode::NotAuthorized);
    assert!(
        api_rejected.message.contains("Rotate it")
            && api_rejected
                .message
                .contains("agentic-org repo set-credential --repo private-app"),
        "{}",
        api_rejected.message
    );
    assert!(!api_rejected.message.contains(secret));

    // Sanitized status reason never exposes raw stderr/tokens.
    let reason =
        sanitized_remote_git_reason(&GitError::RemoteUnavailable(stderr), Some(&remote_unauth));
    assert!(reason.contains("HTTPS authentication required"), "{reason}");
    assert!(!reason.contains(secret));
    assert!(!reason.contains(basic));

    // `scrub_git_log_text` scrubs `GIT_CONFIG_VALUE_<N>=...` in all cases.
    let scrubbed = scrub_git_log_text(&format!(
        "env: GIT_CONFIG_VALUE_0=Authorization: Basic {basic}\ngit_config_value_1=Authorization: Basic {secret}"
    ));
    assert!(!scrubbed.contains(basic), "leaked basic header: {scrubbed}");
    assert!(!scrubbed.contains(secret), "leaked secret: {scrubbed}");
}
