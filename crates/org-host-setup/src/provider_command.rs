//! Native CLI commands for provider connections, office-local bindings, and model profiles
//! (`TASK-120`): `provider create|list|bind|bindings|models` over grant-bound `host.sock` office
//! channels.

use org_proto::hiring::{
    ModelProfileList, ModelProfileListParams, ModelProfileListResult, ProviderConnectionCreate,
    ProviderConnectionCreateParams,
};
use org_proto::providers::{
    DiscoveredModelEntitlement, DiscoveredModelInput, ProviderBindingList,
    ProviderBindingListParams, ProviderBindingListResult, ProviderBindingSet,
    ProviderBindingSetParams, ProviderBindingSetResult, ProviderConnectionList,
    ProviderConnectionListParams, ProviderConnectionListResult, ProviderConnectionRefresh,
    ProviderConnectionRefreshParams, ProviderConnectionRefreshResult,
};
use serde::Serialize;

use super::project_command::call_office;
use org_host::cli_error::{office_refused, HostCliError};

/// Help text for `agentic-org provider`.
pub const HELP: &str = "agentic-org provider <subcommand>\n\
Use when: registering org-wide AI provider connections on HQ, binding local CLI runtimes on HQ or satellite offices, refreshing office readiness observations, or listing available model profiles.\n\n\
Subcommands:\n  \
create <name> --key <key> --provider <type> --runtime <claude|codex|agy> [--auth-mode <subscription_cli|api_key|cloud_iam>] [--subscription-id <id>] [--max-turns <n>] [--org <org_id>] [--login <login>] [--name <instance>] [--json]\n      \
Create an org-wide provider connection and auto-generated model profile on HQ\n      \
Use when: enabling Claude, Codex, or Agy for agents in an organization.\n  \
list [--include-archived] [--org <org_id>] [--login <login>] [--name <instance>] [--json]\n      \
List org-wide provider connections\n      \
Use when: finding a provider `connection_id` before binding it on an office.\n  \
bind <connection_id> --auth-ref <ref> [--binary <path>] [--account-ref <ref>] [--office <office_id>] [--org <org_id>] [--login <login>] [--name <instance>] [--json]\n      \
Bind a provider connection to this local HQ or satellite office\n      \
Use when: configuring a headless Linux satellite or HQ office to run local `claude`, `codex`, or `agy` turns.\n  \
refresh <connection_id> --driver-version <ver> --model <id[:driver_id]> [--account-ref <ref>] [--ttl <secs>] [--office <office_id>] [--org <org_id>] [--login <login>] [--name <instance>] [--json]\n      \
Record a fresh local readiness and discovered-model observation for a bound provider connection\n      \
Use when: marking a headless office's local runtime binding ready for agent session routing (accepts comma-separated `--model` entries).\n  \
bindings [--connection <id>] [--office <office_id>] [--org <org_id>] [--login <login>] [--name <instance>] [--json]\n      \
List provider bindings and readiness state on this office\n      \
Use when: verifying that an office has a ready local binding for a provider connection.\n  \
models [--org <org_id>] [--office <office_id>] [--login <login>] [--name <instance>] [--json]\n      \
List model profiles available for agent hiring\n      \
Use when: finding a `model_profile_id` before hiring an agent with `agentic-org hr hire`.\n\n\
When --login is omitted, uses the local authless session if authentication is disabled,\n\
or prints a browser approval link to stderr when authentication is enabled.";

/// Input for `agentic-org provider create`.
#[derive(Debug, Clone)]
pub struct ProviderCreateInput<'a> {
    pub org_id: Option<&'a str>,
    pub name: &'a str,
    pub key: &'a str,
    pub provider_type: &'a str,
    pub runtime_driver: &'a str,
    pub auth_mode: &'a str,
    pub subscription_identity: Option<&'a str>,
    pub max_concurrent_turns: u64,
}

/// Input for `agentic-org provider bind`.
#[derive(Debug, Clone)]
pub struct ProviderBindInput<'a> {
    pub org_id: Option<&'a str>,
    pub office_id: Option<&'a str>,
    pub connection_id: &'a str,
    pub auth_context_ref: &'a str,
    pub runtime_binary_path: Option<&'a str>,
    pub account_identity_ref: Option<&'a str>,
}

/// Input for `agentic-org provider refresh`.
#[derive(Debug, Clone)]
pub struct ProviderRefreshInput<'a> {
    pub org_id: Option<&'a str>,
    pub office_id: Option<&'a str>,
    pub connection_id: &'a str,
    pub driver_version: &'a str,
    pub models: Vec<(String, String)>,
    pub account_identity_ref: Option<&'a str>,
    pub ttl_seconds: Option<u64>,
}

/// What `agentic-org provider create` made. Serializes as the command's earlier JSON (its
/// `json!` map's key order); `name` and `key` are the trimmed input, for the summary, and
/// are not serialized.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProviderCreated {
    pub connection_id: String,
    pub default_profile_id: String,
    pub generation: u64,
    pub revision: u64,
    #[serde(skip)]
    pub name: String,
    #[serde(skip)]
    pub key: String,
}

/// `agentic-org provider create`
pub fn provider_create(
    instance_name: Option<&str>,
    login: Option<&str>,
    input: &ProviderCreateInput<'_>,
) -> Result<ProviderCreated, HostCliError> {
    let command_id = org_host::ids::uuid_v7().map_err(|_| "could not generate command ID")?;
    let connection_id = org_host::ids::uuid_v7().map_err(|_| "could not generate connection ID")?;
    let default_profile_id =
        org_host::ids::uuid_v7().map_err(|_| "could not generate model profile ID")?;
    let auth_mode = input.auth_mode.trim();
    let subscription_identity = match input.subscription_identity {
        Some(raw) if !raw.trim().is_empty() => Some(raw.trim().to_owned()),
        _ if auth_mode == "subscription_cli" => Some(format!("sub:{}", input.key.trim())),
        _ => None,
    };
    let params = ProviderConnectionCreateParams {
        command_id,
        connection_id: connection_id.clone(),
        default_profile_id: default_profile_id.clone(),
        key: input.key.trim().to_owned(),
        name: input.name.trim().to_owned(),
        provider_type: input.provider_type.trim().to_owned(),
        auth_mode: auth_mode.to_owned(),
        subscription_identity,
        runtime_driver: input.runtime_driver.trim().to_owned(),
        endpoint_url: None,
        max_concurrent_turns: input.max_concurrent_turns.max(1),
    };
    let created = call_office::<ProviderConnectionCreate>(
        instance_name,
        login,
        input.org_id,
        None,
        true,
        &params,
    )?
    .map_err(|err| office_refused("creating provider connection", &err))?;
    Ok(ProviderCreated {
        connection_id,
        default_profile_id,
        generation: created.generation,
        revision: created.revision,
        name: params.name,
        key: params.key,
    })
}

/// `agentic-org provider list`
pub fn provider_list(
    instance_name: Option<&str>,
    login: Option<&str>,
    org_id: Option<&str>,
    include_archived: bool,
) -> Result<ProviderConnectionListResult, HostCliError> {
    call_office::<ProviderConnectionList>(
        instance_name,
        login,
        org_id,
        None,
        false,
        &ProviderConnectionListParams { include_archived },
    )?
    .map_err(|err| office_refused("listing provider connections", &err))
}

/// `agentic-org provider bind`
pub fn provider_bind(
    instance_name: Option<&str>,
    login: Option<&str>,
    input: &ProviderBindInput<'_>,
) -> Result<ProviderBindingSetResult, HostCliError> {
    let operation_id = org_host::ids::uuid_v7().map_err(|_| "could not generate operation ID")?;
    let params = ProviderBindingSetParams {
        operation_id,
        connection_id: input.connection_id.trim().to_owned(),
        expected_revision: None,
        auth_context_ref: input.auth_context_ref.trim().to_owned(),
        runtime_binary_path: input.runtime_binary_path.map(|s| s.trim().to_owned()),
        account_identity_ref: input.account_identity_ref.map(|s| s.trim().to_owned()),
    };
    call_office::<ProviderBindingSet>(
        instance_name,
        login,
        input.org_id,
        input.office_id,
        false,
        &params,
    )?
    .map_err(|err| office_refused("binding provider connection", &err))
}

/// `agentic-org provider refresh`
pub fn provider_refresh(
    instance_name: Option<&str>,
    login: Option<&str>,
    input: &ProviderRefreshInput<'_>,
) -> Result<ProviderConnectionRefreshResult, HostCliError> {
    let models = input
        .models
        .iter()
        .map(|(model_id, driver_model_id)| DiscoveredModelInput {
            model_id: model_id.clone(),
            driver_model_id: driver_model_id.clone(),
            entitlement: DiscoveredModelEntitlement::Confirmed,
            capabilities_json: "{}".into(),
        })
        .collect();
    let params = ProviderConnectionRefreshParams {
        connection_id: input.connection_id.trim().to_owned(),
        driver_version: Some(input.driver_version.trim().to_owned()),
        auth_signed_in: Some(true),
        observed_subscription_identity: input.account_identity_ref.map(|s| s.trim().to_owned()),
        operator_attested_subscription: true,
        models,
        issue_code: None,
        ttl_seconds: input.ttl_seconds,
    };
    call_office::<ProviderConnectionRefresh>(
        instance_name,
        login,
        input.org_id,
        input.office_id,
        false,
        &params,
    )?
    .map_err(|err| office_refused("refreshing provider connection", &err))
}

/// `agentic-org provider bindings`
pub fn provider_bindings(
    instance_name: Option<&str>,
    login: Option<&str>,
    org_id: Option<&str>,
    office_id: Option<&str>,
    connection_id: Option<&str>,
) -> Result<ProviderBindingListResult, HostCliError> {
    call_office::<ProviderBindingList>(
        instance_name,
        login,
        org_id,
        office_id,
        false,
        &ProviderBindingListParams {
            connection_id: connection_id.map(str::to_owned),
        },
    )?
    .map_err(|err| office_refused("listing office provider bindings", &err))
}

/// `agentic-org provider models`
pub fn provider_models(
    instance_name: Option<&str>,
    login: Option<&str>,
    org_id: Option<&str>,
    office_id: Option<&str>,
) -> Result<ModelProfileListResult, HostCliError> {
    call_office::<ModelProfileList>(
        instance_name,
        login,
        org_id,
        office_id,
        false,
        &ModelProfileListParams {},
    )?
    .map_err(|err| office_refused("listing model profiles", &err))
}

#[cfg(test)]
mod tests {
    use super::HELP;

    #[test]
    fn provider_help_covers_every_subcommand_with_use_when() {
        for sub in ["create", "list", "bind", "refresh", "bindings", "models"] {
            assert!(HELP.contains(sub), "provider HELP missing `{sub}`");
        }
        assert!(
            HELP.matches("Use when:").count() >= 7,
            "provider HELP missing Use when cues:\n{HELP}"
        );
    }
}
