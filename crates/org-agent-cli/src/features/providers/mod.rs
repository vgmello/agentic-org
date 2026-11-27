//! The providers & model-profiles slice: `provider list|quota` and `model-profile list`
//! (`TASK-54`, `TASK-80.4`, `doc-88`).

use org_command::{Answer, CommandTable, Refusal, Request};
use org_proto::hiring::{ModelProfileList, ModelProfileListParams, ModelProfileListResult};
use org_proto::providers::{
    ProviderConnectionList, ProviderConnectionListParams, ProviderConnectionListResult,
    ProviderQuotaStatus, ProviderQuotaStatusParams, ProviderQuotaStatusResult,
};
use serde_json::Value;

use crate::runtime::domain::{CommandCue, Domain};
use crate::runtime::input::{call_rpc, encode, is_set, opt_string};
use crate::runtime::transport::{paginate, parse_cursor, OfficeTransport};

/// The providers & model-profiles slice as the registry composes it.
pub(crate) const DOMAIN: Domain = Domain {
    commands: org_command_catalogue::providers::ALL,
    resources: &[
        (
            "provider",
            "listing organization provider connections and inspecting live pool cooldowns and turn permits.",
        ),
        (
            "model-profile",
            "listing model profiles and their ordered failover provider connections.",
        ),
    ],
    cues: &[CUES],
    dispatch,
};

const CUES: &[CommandCue] = &[
    (
        "provider",
        "list",
        "listing configured provider connections, concurrency caps, and enabled/archived status.",
    ),
    (
        "provider",
        "quota",
        "checking active provider cooldowns and turn permits when diagnosing rate limits or pool exhaustion.",
    ),
    (
        "model-profile",
        "list",
        "listing available model profiles and their ordered failover connection pools.",
    ),
];

fn dispatch(
    transport: &mut dyn OfficeTransport,
    request: &Request,
    _table: &CommandTable,
) -> Option<Result<Answer, Refusal>> {
    match (request.command.resource, request.command.verb) {
        ("provider", "list") => Some(read_provider_list(transport, request)),
        ("provider", "quota") => Some(read_provider_quota(transport, request)),
        ("model-profile", "list") => Some(read_model_profile_list(transport, request)),
        _ => None,
    }
}

fn read_provider_list<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    parse_cursor(request.cursor.as_deref())?;
    let params = ProviderConnectionListParams {
        include_archived: is_set(request, "include_archived"),
    };
    let res: ProviderConnectionListResult =
        call_rpc::<ProviderConnectionList, _>(transport, &params)?;
    let items = match encode(&res.connections, "provider.connection.list")? {
        Value::Array(items) => items,
        _ => Vec::new(),
    };
    paginate(request, items)
}

fn read_provider_quota<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let params = ProviderQuotaStatusParams {
        connection_id: opt_string(request, "connection_id"),
    };
    let res: ProviderQuotaStatusResult = call_rpc::<ProviderQuotaStatus, _>(transport, &params)?;
    Ok(Answer::completed(encode(&res, "provider.quota.status")?))
}

fn read_model_profile_list<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    parse_cursor(request.cursor.as_deref())?;
    let res: ModelProfileListResult =
        call_rpc::<ModelProfileList, _>(transport, &ModelProfileListParams {})?;
    let items = match encode(&res.profiles, "model_profile.list")? {
        Value::Array(items) => items,
        _ => Vec::new(),
    };
    paginate(request, items)
}

#[cfg(test)]
mod tests {
    use serde_json::{json, Value};

    use crate::execute_with_office;
    use crate::runtime::transport::{OfficeTransport, TransportOffice};

    struct StubProviderTransport {
        calls: Vec<(String, Value)>,
    }

    impl OfficeTransport for StubProviderTransport {
        fn call_method(
            &mut self,
            method: &str,
            params: Value,
        ) -> Result<Value, org_command::Refusal> {
            self.calls.push((method.to_owned(), params));
            match method {
                "provider.connection.list" => Ok(json!({ "connections": [] })),
                "provider.quota.status" => Ok(json!({
                    "coordinator_generation": 1,
                    "cooldowns": [],
                    "active_permits": [],
                    "release_pending_permits": []
                })),
                "model_profile.list" => Ok(json!({ "profiles": [] })),
                other => panic!("unexpected method `{other}`"),
            }
        }
    }

    #[test]
    fn provider_and_model_profile_commands_dispatch_expected_rpcs() {
        let mut office = TransportOffice(StubProviderTransport { calls: Vec::new() });
        for line in [
            &["provider", "list", "--include-archived"][..],
            &["provider", "quota", "--connection-id", "c1"],
            &["model-profile", "list"],
        ] {
            let args: Vec<String> = line.iter().map(|s| (*s).to_owned()).collect();
            let out = execute_with_office("agent-os", &args, &mut office);
            assert_eq!(out.exit_code, 0, "{line:?}: {}", out.stdout);
        }
        let methods: Vec<&str> = office.0.calls.iter().map(|(m, _)| m.as_str()).collect();
        assert_eq!(
            methods,
            vec![
                "provider.connection.list",
                "provider.quota.status",
                "model_profile.list",
            ]
        );
    }
}
