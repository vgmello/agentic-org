//! Native CLI commands for provider connections, office-local bindings, and model profiles
//! (`TASK-120`): `provider create|list|bind|bindings|models` over grant-bound `host.sock` office
//! channels.

use std::io::Write;

use org_proto::hiring::{
    ModelProfileList, ModelProfileListParams, ProviderConnectionCreate,
    ProviderConnectionCreateParams,
};
use org_proto::providers::{
    DiscoveredModelEntitlement, DiscoveredModelInput, ProviderBindingList,
    ProviderBindingListParams, ProviderBindingSet, ProviderBindingSetParams,
    ProviderConnectionList, ProviderConnectionListParams, ProviderConnectionRefresh,
    ProviderConnectionRefreshParams,
};
use org_proto::{ApiError, Call};

use super::office_command::{connect_cli_session, end_session, Target};
use super::project_command::OfficeSession;
use super::{registered, say};

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
    pub json: bool,
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
    pub json: bool,
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
    pub json: bool,
}

fn run_provider_rpc<C: Call>(
    instance_name: Option<&str>,
    login: Option<&str>,
    org_id: Option<&str>,
    office_id: Option<&str>,
    require_hq: bool,
    params: &C::Params,
) -> Result<Result<C::Result, ApiError>, String> {
    let (_, id, socket) = registered(&crate::human::paths(instance_name)?, instance_name)?;
    let target = Target {
        socket: &socket,
        instance_id: &id,
        name: instance_name,
    };
    let mut err_out = std::io::stderr().lock();
    let (mut wire, session) =
        connect_cli_session(&target, login, &mut crate::human::password, &mut err_out)?;
    let answer = (|| {
        let mut office = if require_hq {
            OfficeSession::open_hq(&mut wire, &session, org_id)?
        } else {
            OfficeSession::open_for_office(&mut wire, &session, org_id, office_id)?
        };
        office.call::<C>(&mut wire, &session, params)
    })();
    end_session(&mut wire, &session);
    answer
}

fn office_error(action: &str, error: ApiError) -> String {
    format!(
        "the office refused {action} ({:?}): {}",
        error.code, error.message
    )
}

/// `agentic-org provider create`
pub fn provider_create(
    instance_name: Option<&str>,
    login: Option<&str>,
    input: &ProviderCreateInput<'_>,
    out: &mut dyn Write,
) -> Result<(), String> {
    let command_id = crate::ids::uuid_v7().map_err(|_| "could not generate command ID")?;
    let connection_id = crate::ids::uuid_v7().map_err(|_| "could not generate connection ID")?;
    let default_profile_id =
        crate::ids::uuid_v7().map_err(|_| "could not generate model profile ID")?;
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
    let created = match run_provider_rpc::<ProviderConnectionCreate>(
        instance_name,
        login,
        input.org_id,
        None,
        true,
        &params,
    )? {
        Ok(res) => res,
        Err(err) => return Err(office_error("creating provider connection", err)),
    };
    if input.json {
        let body = serde_json::json!({
            "connection_id": connection_id,
            "default_profile_id": default_profile_id,
            "revision": created.revision,
            "generation": created.generation,
        });
        let encoded = serde_json::to_string_pretty(&body)
            .map_err(|_| "could not encode provider create response".to_owned())?;
        return say(out, encoded);
    }
    say(
        out,
        format!(
            "Created provider connection \"{}\" (key={})\n  connection_id:      {connection_id}\n  default_profile_id: {default_profile_id}\n  revision:           {}",
            input.name.trim(),
            input.key.trim(),
            created.revision
        ),
    )
}

/// `agentic-org provider list`
pub fn provider_list(
    instance_name: Option<&str>,
    login: Option<&str>,
    org_id: Option<&str>,
    include_archived: bool,
    json: bool,
    out: &mut dyn Write,
) -> Result<(), String> {
    let listed = match run_provider_rpc::<ProviderConnectionList>(
        instance_name,
        login,
        org_id,
        None,
        false,
        &ProviderConnectionListParams { include_archived },
    )? {
        Ok(res) => res,
        Err(err) => return Err(office_error("listing provider connections", err)),
    };
    if json {
        let encoded = serde_json::to_string_pretty(&listed)
            .map_err(|_| "could not encode provider list".to_owned())?;
        return say(out, encoded);
    }
    if listed.connections.is_empty() {
        return say(out, "No provider connections.");
    }
    for conn in &listed.connections {
        say(
            out,
            format!(
                "{}\tkey={}\truntime={}\tprovider={}\tenabled={}\tname={}",
                conn.id, conn.key, conn.runtime_driver, conn.provider_type, conn.enabled, conn.name
            ),
        )?;
    }
    Ok(())
}

/// `agentic-org provider bind`
pub fn provider_bind(
    instance_name: Option<&str>,
    login: Option<&str>,
    input: &ProviderBindInput<'_>,
    out: &mut dyn Write,
) -> Result<(), String> {
    let operation_id = crate::ids::uuid_v7().map_err(|_| "could not generate operation ID")?;
    let params = ProviderBindingSetParams {
        operation_id,
        connection_id: input.connection_id.trim().to_owned(),
        expected_revision: None,
        auth_context_ref: input.auth_context_ref.trim().to_owned(),
        runtime_binary_path: input.runtime_binary_path.map(|s| s.trim().to_owned()),
        account_identity_ref: input.account_identity_ref.map(|s| s.trim().to_owned()),
    };
    let bound = match run_provider_rpc::<ProviderBindingSet>(
        instance_name,
        login,
        input.org_id,
        input.office_id,
        false,
        &params,
    )? {
        Ok(res) => res,
        Err(err) => return Err(office_error("binding provider connection", err)),
    };
    if input.json {
        let encoded = serde_json::to_string_pretty(&bound)
            .map_err(|_| "could not encode provider binding response".to_owned())?;
        return say(out, encoded);
    }
    say(
        out,
        format!(
            "Bound connection {} on office {} (revision={}, auth_ref={}).",
            bound.binding.connection_id,
            bound.binding.office_id,
            bound.binding.revision,
            bound.binding.auth_context_ref
        ),
    )
}

/// `agentic-org provider refresh`
pub fn provider_refresh(
    instance_name: Option<&str>,
    login: Option<&str>,
    input: &ProviderRefreshInput<'_>,
    out: &mut dyn Write,
) -> Result<(), String> {
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
    let refreshed = match run_provider_rpc::<ProviderConnectionRefresh>(
        instance_name,
        login,
        input.org_id,
        input.office_id,
        false,
        &params,
    )? {
        Ok(res) => res,
        Err(err) => return Err(office_error("refreshing provider connection", err)),
    };
    if input.json {
        let encoded = serde_json::to_string_pretty(&refreshed)
            .map_err(|_| "could not encode provider refresh response".to_owned())?;
        return say(out, encoded);
    }
    say(
        out,
        format!(
            "Refreshed connection {} on office {}: status={} models={}",
            refreshed.state.connection_id,
            refreshed.state.office_id,
            refreshed.state.status.as_str(),
            refreshed.models.len()
        ),
    )
}

/// `agentic-org provider bindings`
pub fn provider_bindings(
    instance_name: Option<&str>,
    login: Option<&str>,
    org_id: Option<&str>,
    office_id: Option<&str>,
    connection_id: Option<&str>,
    json: bool,
    out: &mut dyn Write,
) -> Result<(), String> {
    let listed = match run_provider_rpc::<ProviderBindingList>(
        instance_name,
        login,
        org_id,
        office_id,
        false,
        &ProviderBindingListParams {
            connection_id: connection_id.map(str::to_owned),
        },
    )? {
        Ok(res) => res,
        Err(err) => return Err(office_error("listing office provider bindings", err)),
    };
    if json {
        let encoded = serde_json::to_string_pretty(&listed)
            .map_err(|_| "could not encode provider bindings".to_owned())?;
        return say(out, encoded);
    }
    if listed.bindings.is_empty() {
        return say(out, "No local provider bindings.");
    }
    for entry in &listed.bindings {
        let status = entry
            .state
            .as_ref()
            .map(|s| s.status.as_str())
            .unwrap_or("unknown");
        say(
            out,
            format!(
                "office={}\tconnection={}\trev={}\tstatus={status}\tauth_ref={}",
                entry.binding.office_id,
                entry.binding.connection_id,
                entry.binding.revision,
                entry.binding.auth_context_ref
            ),
        )?;
    }
    Ok(())
}

/// `agentic-org provider models`
pub fn provider_models(
    instance_name: Option<&str>,
    login: Option<&str>,
    org_id: Option<&str>,
    office_id: Option<&str>,
    json: bool,
    out: &mut dyn Write,
) -> Result<(), String> {
    let listed = match run_provider_rpc::<ModelProfileList>(
        instance_name,
        login,
        org_id,
        office_id,
        false,
        &ModelProfileListParams {},
    )? {
        Ok(res) => res,
        Err(err) => return Err(office_error("listing model profiles", err)),
    };
    if json {
        let encoded = serde_json::to_string_pretty(&listed)
            .map_err(|_| "could not encode model profiles".to_owned())?;
        return say(out, encoded);
    }
    if listed.profiles.is_empty() {
        return say(out, "No model profiles.");
    }
    for profile in &listed.profiles {
        say(
            out,
            format!(
                "{}\tkey={}\tconnections={}\tname={}",
                profile.id,
                profile.key,
                profile.connection_ids.join(","),
                profile.name
            ),
        )?;
    }
    Ok(())
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
