use super::*;

const ORG: &str = "01J0000000000000000000ORG1";
const OFFICE: &str = "01J000000000000000000OFFC1";
const KEY: [u8; 32] = [7; 32];

fn binding<'a>(organization_id: &'a str, office_id: &'a str) -> WrapBinding<'a> {
    WrapBinding {
        organization_id,
        office_id,
        purpose: WrapPurpose::OrganizationCa,
    }
}

fn secret() -> Vec<u8> {
    crate::SigningKey::generate().unwrap().local_der().to_vec()
}

#[test]
fn round_trip_returns_the_same_key_and_hides_it() {
    let der = secret();
    let record = wrap(&KEY, &binding(ORG, OFFICE), &der).unwrap();
    assert_eq!(record[0], 1);
    assert_eq!(record.len(), 1 + 12 + der.len() + 16);
    assert!(!record.windows(der.len()).any(|w| w == der.as_slice()));
    assert_eq!(*unwrap(&KEY, &binding(ORG, OFFICE), &record).unwrap(), der);
    // A fresh nonce each time: the same input never yields the same record.
    assert_ne!(record, wrap(&KEY, &binding(ORG, OFFICE), &der).unwrap());
}

#[test]
fn enc6_record_for_one_binding_or_key_fails_for_another() {
    let der = secret();
    let record = wrap(&KEY, &binding(ORG, OFFICE), &der).unwrap();
    for (key, organization, office) in [
        ([8; 32], ORG, OFFICE),
        (KEY, "01J0000000000000000000ORG2", OFFICE),
        (KEY, ORG, "01J000000000000000000OFFC2"),
        // Field boundaries are length-prefixed, so shifting bytes between fields fails.
        (
            KEY,
            "01J0000000000000000000ORG10",
            "1J000000000000000000OFFC1",
        ),
    ] {
        assert_eq!(
            unwrap(&key, &binding(organization, office), &record),
            Err(WrapError::Rejected)
        );
    }
}

#[test]
fn enc6_purpose_is_bound_into_the_authenticated_data() {
    let mut other = aad(&binding(ORG, OFFICE)).unwrap();
    let purpose = WrapPurpose::OrganizationCa.label();
    let start = DOMAIN.len() + 2;
    assert_eq!(&other[start..start + purpose.len()], purpose);
    // Any different purpose label changes the authenticated data, so the tag fails.
    other[start] ^= 1;
    let der = secret();
    let record = wrap(&KEY, &binding(ORG, OFFICE), &der).unwrap();
    let nonce = Nonce::assume_unique_for_key(record[1..13].try_into().unwrap());
    let mut body = record[13..].to_vec();
    assert!(cipher(&KEY)
        .unwrap()
        .open_in_place(nonce, Aad::from(other), &mut body)
        .is_err());
}

#[test]
fn enc6_truncated_tampered_or_reversioned_records_are_rejected() {
    let der = secret();
    let record = wrap(&KEY, &binding(ORG, OFFICE), &der).unwrap();
    let b = binding(ORG, OFFICE);
    for cut in [0, 1, 13, 29, record.len() - 1] {
        assert_eq!(
            unwrap(&KEY, &b, &record[..cut]),
            Err(WrapError::Rejected),
            "{cut}"
        );
    }
    for version in [0u8, 2, 255] {
        let mut changed = record.clone();
        changed[0] = version;
        assert_eq!(unwrap(&KEY, &b, &changed), Err(WrapError::Rejected));
    }
    for at in [1, 13, record.len() - 1] {
        let mut changed = record.clone();
        changed[at] ^= 1;
        assert_eq!(unwrap(&KEY, &b, &changed), Err(WrapError::Rejected), "{at}");
    }
}

#[test]
fn out_of_bounds_inputs_are_refused_before_any_crypto() {
    let der = secret();
    assert_eq!(
        wrap(&KEY, &binding("", OFFICE), &der),
        Err(WrapError::Invalid)
    );
    assert_eq!(wrap(&KEY, &binding(ORG, ""), &der), Err(WrapError::Invalid));
    let long = "a".repeat(256);
    assert_eq!(
        wrap(&KEY, &binding(&long, OFFICE), &der),
        Err(WrapError::Invalid)
    );
    assert_eq!(
        wrap(&KEY, &binding(ORG, OFFICE), &[]),
        Err(WrapError::Invalid)
    );
    assert_eq!(
        wrap(&KEY, &binding(ORG, OFFICE), &vec![1; MAX_KEY_BYTES + 1]),
        Err(WrapError::Invalid)
    );
    let record = wrap(&KEY, &binding(ORG, OFFICE), &der).unwrap();
    assert_eq!(
        unwrap(&KEY, &binding("", OFFICE), &record),
        Err(WrapError::Invalid)
    );
}

#[test]
fn task_11_ca_and_provider_credentials_use_distinct_wrapping_keys_and_domains() {
    let ca_key = derive_wrapping_key(&KEY, KeyClass::OfficePrivateKey).unwrap();
    let provider_key = derive_wrapping_key(&KEY, KeyClass::ProviderCredential).unwrap();

    // Derived keys are deterministic, distinct from each other, and distinct from the master key.
    assert_eq!(
        *ca_key,
        *derive_wrapping_key(&KEY, KeyClass::OfficePrivateKey).unwrap()
    );
    assert_eq!(
        *provider_key,
        *derive_wrapping_key(&KEY, KeyClass::ProviderCredential).unwrap()
    );
    assert_ne!(*ca_key, *provider_key);
    assert_ne!(*ca_key, KEY);
    assert_ne!(*provider_key, KEY);

    let conn_binding = ProviderCredentialBinding {
        organization_id: ORG,
        office_id: OFFICE,
        connection_id: "01J000000000000000000CONN1",
    };
    let token = b"sk-ant-oat01-example-oauth-token";
    let wrapped = wrap_provider_credential(&provider_key, &conn_binding, token).unwrap();
    assert!(!wrapped.windows(token.len()).any(|w| w == token));
    assert_eq!(
        *unwrap_provider_credential(&provider_key, &conn_binding, &wrapped).unwrap(),
        token
    );

    // Cross-key and cross-domain unwrapping fails closed.
    assert_eq!(
        unwrap_provider_credential(&ca_key, &conn_binding, &wrapped),
        Err(WrapError::Rejected)
    );
    assert_eq!(
        unwrap_provider_credential(&KEY, &conn_binding, &wrapped),
        Err(WrapError::Rejected)
    );
    assert_eq!(
        unwrap(&provider_key, &binding(ORG, OFFICE), &wrapped),
        Err(WrapError::Rejected)
    );
    assert_eq!(
        unwrap_provider_credential(
            &provider_key,
            &ProviderCredentialBinding {
                connection_id: "01J000000000000000000CONN2",
                ..conn_binding
            },
            &wrapped
        ),
        Err(WrapError::Rejected)
    );
}

#[test]
fn task_135_git_credentials_use_distinct_key_class_and_strict_repo_scope_binding() {
    let ca_key = derive_wrapping_key(&KEY, KeyClass::OfficePrivateKey).unwrap();
    let provider_key = derive_wrapping_key(&KEY, KeyClass::ProviderCredential).unwrap();
    let git_key = derive_wrapping_key(&KEY, KeyClass::GitCredential).unwrap();

    assert_ne!(*git_key, *ca_key);
    assert_ne!(*git_key, *provider_key);
    assert_ne!(*git_key, KEY);

    let binding = GitCredentialBinding {
        organization_id: ORG,
        office_id: OFFICE,
        repository_id: "01950000-0000-7000-8000-000000000301",
        scope_url: "https://github.com/acme/private-repo.git",
    };
    let secret = b"{\"username\":\"x-access-token\",\"token\":\"ghp_SCOPED123\"}";
    let wrapped = wrap_git_credential(&git_key, &binding, secret).unwrap();
    assert!(!wrapped.windows(secret.len()).any(|w| w == secret));
    assert_eq!(
        *unwrap_git_credential(&git_key, &binding, &wrapped).unwrap(),
        secret
    );

    // Wrong key class, wrong repo ID, wrong scope URL, or empty scope URL fail closed.
    assert_eq!(
        unwrap_git_credential(&provider_key, &binding, &wrapped),
        Err(WrapError::Rejected)
    );
    assert_eq!(
        unwrap_git_credential(
            &git_key,
            &GitCredentialBinding {
                repository_id: "01950000-0000-7000-8000-000000000302",
                ..binding
            },
            &wrapped
        ),
        Err(WrapError::Rejected)
    );
    assert_eq!(
        unwrap_git_credential(
            &git_key,
            &GitCredentialBinding {
                scope_url: "https://github.com/acme/other-repo.git",
                ..binding
            },
            &wrapped
        ),
        Err(WrapError::Rejected)
    );
    assert_eq!(
        wrap_git_credential(
            &git_key,
            &GitCredentialBinding {
                scope_url: "",
                ..binding
            },
            secret
        ),
        Err(WrapError::Invalid)
    );
}

#[test]
fn task272_github_app_key_class_and_aad_binding_are_isolated() {
    let git_key = derive_wrapping_key(&KEY, KeyClass::GitCredential).unwrap();
    let provider_key = derive_wrapping_key(&KEY, KeyClass::ProviderCredential).unwrap();
    let office_key = derive_wrapping_key(&KEY, KeyClass::OfficePrivateKey).unwrap();
    let github_key = derive_wrapping_key(&KEY, KeyClass::GitHubApp).unwrap();
    assert_ne!(*github_key, *git_key);
    assert_ne!(*github_key, *provider_key);
    assert_ne!(*github_key, *office_key);

    let binding = GitHubAppCredentialBinding {
        organization_id: ORG,
        office_id: OFFICE,
        app_id: 424_242,
        purpose: "private-key",
    };
    let secret = b"synthetic-github-app-key-material";
    let wrapped = wrap_github_app_credential(&github_key, &binding, secret).unwrap();
    assert!(!wrapped.windows(secret.len()).any(|w| w == secret));
    assert_eq!(
        *unwrap_github_app_credential(&github_key, &binding, &wrapped).unwrap(),
        secret
    );

    // Wrong key class, wrong org, wrong office, wrong app_id, or swapped purpose fail closed.
    assert_eq!(
        unwrap_github_app_credential(&git_key, &binding, &wrapped),
        Err(WrapError::Rejected)
    );
    assert_eq!(
        unwrap_github_app_credential(
            &github_key,
            &GitHubAppCredentialBinding {
                organization_id: "01J0000000000000000000ORG2",
                ..binding
            },
            &wrapped
        ),
        Err(WrapError::Rejected)
    );
    assert_eq!(
        unwrap_github_app_credential(
            &github_key,
            &GitHubAppCredentialBinding {
                office_id: "01J000000000000000000OFFC2",
                ..binding
            },
            &wrapped
        ),
        Err(WrapError::Rejected)
    );
    assert_eq!(
        unwrap_github_app_credential(
            &github_key,
            &GitHubAppCredentialBinding {
                app_id: 424_243,
                ..binding
            },
            &wrapped
        ),
        Err(WrapError::Rejected)
    );
    assert_eq!(
        unwrap_github_app_credential(
            &github_key,
            &GitHubAppCredentialBinding {
                purpose: "webhook-secret",
                ..binding
            },
            &wrapped
        ),
        Err(WrapError::Rejected)
    );
    assert_eq!(
        wrap_github_app_credential(
            &github_key,
            &GitHubAppCredentialBinding {
                app_id: 0,
                ..binding
            },
            secret
        ),
        Err(WrapError::Invalid)
    );
    assert_eq!(
        wrap_github_app_credential(
            &github_key,
            &GitHubAppCredentialBinding {
                purpose: "",
                ..binding
            },
            secret
        ),
        Err(WrapError::Invalid)
    );
}
