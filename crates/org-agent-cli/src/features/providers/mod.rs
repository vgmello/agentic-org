//! The providers & model-profiles slice: `provider list|quota` and `model-profile list`
//! (`TASK-54`, `TASK-80.4`, `doc-88`).

use org_command::{Answer, Command, CommandTable, Example, Field, Kind, Refusal, Request, Type};
use org_proto::hiring::{ModelProfileList, ModelProfileListParams, ModelProfileListResult};
use org_proto::providers::{
    ProviderConnectionList, ProviderConnectionListParams, ProviderConnectionListResult,
    ProviderQuotaStatus, ProviderQuotaStatusParams, ProviderQuotaStatusResult,
};
use org_proto::ErrorCode;
use serde_json::Value;

use crate::runtime::domain::{CommandCue, Domain};
use crate::runtime::input::{call_rpc, encode, is_set, opt_string};
use crate::runtime::transport::{paginate, parse_cursor, OfficeTransport};

/// The providers & model-profiles slice as the registry composes it.
pub(crate) const DOMAIN: Domain = Domain {
    commands: &[COMMANDS],
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

const READ_ERRORS: &[ErrorCode] = &[
    ErrorCode::InvalidRequest,
    ErrorCode::NotBound,
    ErrorCode::NotFound,
    ErrorCode::NotAuthorized,
    ErrorCode::Unavailable,
];

const STRING_TYPE: Type = Type::String;

const PROVIDER_LIST_INPUT: &[Field] = &[Field::optional(
    "include_archived",
    Type::Boolean,
    "Include archived provider connections",
)];

const PROVIDER_CONNECTION_FIELDS: &[Field] = &[
    Field::required("id", Type::String, "Provider connection UUIDv7"),
    Field::required("revision", Type::Integer, "Connection revision"),
    Field::required("key", Type::String, "Stable connection key"),
    Field::required("name", Type::String, "Display name"),
    Field::required("provider_type", Type::String, "Provider type"),
    Field::required("auth_mode", Type::String, "Authentication mode"),
    Field::optional(
        "subscription_identity",
        Type::String,
        "Subscription identity, when applicable",
    ),
    Field::required("runtime_driver", Type::String, "Runtime driver name"),
    Field::optional("endpoint_url", Type::String, "Optional custom endpoint URL"),
    Field::required(
        "enabled",
        Type::Boolean,
        "Whether the connection is enabled",
    ),
    Field::required(
        "archived",
        Type::Boolean,
        "Whether the connection is archived",
    ),
    Field::required(
        "max_concurrent_turns",
        Type::Integer,
        "Maximum concurrent turns",
    ),
    Field::required("created_at", Type::String, "Creation timestamp"),
    Field::required("updated_at", Type::String, "Last update timestamp"),
];

const PROVIDER_QUOTA_INPUT: &[Field] = &[Field::optional(
    "connection_id",
    Type::String,
    "Filter cooldowns and permits to one provider connection ID",
)];

const CONNECTION_QUOTA_FIELDS: &[Field] = &[
    Field::required("connection_id", Type::String, "Provider connection UUIDv7"),
    Field::required("scope_key", Type::String, "Quota scope key"),
    Field::optional("model_id", Type::String, "Model identifier when scoped"),
    Field::required("status", Type::String, "unknown, open, or cooldown"),
    Field::optional(
        "cooldown_until",
        Type::String,
        "RFC 3339 cooldown expiry timestamp",
    ),
    Field::required("observed_at", Type::String, "Observation timestamp"),
    Field::required(
        "observed_by_office_id",
        Type::String,
        "Reporting office UUIDv7",
    ),
    Field::required(
        "coordinator_generation",
        Type::Integer,
        "Coordinator generation",
    ),
];
const CONNECTION_QUOTA_TYPE: Type = Type::Object(CONNECTION_QUOTA_FIELDS);

const TURN_PERMIT_FIELDS: &[Field] = &[
    Field::required("id", Type::String, "Turn permit UUIDv7"),
    Field::required("connection_id", Type::String, "Provider connection UUIDv7"),
    Field::required("office_id", Type::String, "Holding office UUIDv7"),
    Field::required("employee_id", Type::String, "Agent employee UUIDv7"),
    Field::required(
        "placement_generation",
        Type::Integer,
        "Agent placement generation",
    ),
    Field::required("model_id", Type::String, "Model identifier"),
    Field::required("authority", Type::String, "hq_pool or office_local"),
    Field::required(
        "state",
        Type::String,
        "active, release_pending, or released",
    ),
    Field::required("issued_at", Type::String, "Permit issue timestamp"),
    Field::required("expires_at", Type::String, "Permit expiry timestamp"),
    Field::required(
        "coordinator_generation",
        Type::Integer,
        "Coordinator generation",
    ),
];
const TURN_PERMIT_TYPE: Type = Type::Object(TURN_PERMIT_FIELDS);

const PROVIDER_QUOTA_OUTPUT: &[Field] = &[
    Field::required(
        "coordinator_generation",
        Type::Integer,
        "Coordinator generation",
    ),
    Field::required(
        "cooldowns",
        Type::List(&CONNECTION_QUOTA_TYPE),
        "Connection cooldown states",
    ),
    Field::required(
        "active_permits",
        Type::List(&TURN_PERMIT_TYPE),
        "Currently active turn permits",
    ),
    Field::required(
        "release_pending_permits",
        Type::List(&TURN_PERMIT_TYPE),
        "Turn permits awaiting release acknowledgement",
    ),
];

const MODEL_PROFILE_FIELDS: &[Field] = &[
    Field::required("id", Type::String, "Model profile UUIDv7"),
    Field::required("key", Type::String, "Stable profile key"),
    Field::required("name", Type::String, "Display name"),
    Field::required("description", Type::String, "Profile description"),
    Field::required(
        "is_auto_generated",
        Type::Boolean,
        "Whether auto-generated for one connection",
    ),
    Field::optional(
        "generated_for_connection_id",
        Type::String,
        "Source connection UUIDv7 if auto-generated",
    ),
    Field::required("archived", Type::Boolean, "Whether the profile is archived"),
    Field::required(
        "connection_ids",
        Type::List(&STRING_TYPE),
        "Ordered failover connection IDs",
    ),
    Field::required("revision", Type::Integer, "Profile revision"),
];

const COMMANDS: &[Command] = &[
    Command {
        resource: "provider",
        verb: "list",
        summary: "List organization provider connections",
        call: "provider.connection.list",
        kind: Kind::List,
        input: PROVIDER_LIST_INPUT,
        content_field: None,
        output: PROVIDER_CONNECTION_FIELDS,
        errors: READ_ERRORS,
        example: Example {
            input: "{}",
            output: r#"[{"id":"0193a5c0-0000-7000-8000-000000000801","revision":1,"key":"primary","name":"Primary Pool","provider_type":"api","auth_mode":"api_key","runtime_driver":"default","enabled":true,"archived":false,"max_concurrent_turns":4,"created_at":"2026-11-01T00:00:00Z","updated_at":"2026-11-01T00:00:00Z"}]"#,
        },
    },
    Command {
        resource: "provider",
        verb: "quota",
        summary: "Inspect provider pool cooldowns and active turn permits",
        call: "provider.quota.status",
        kind: Kind::Read,
        input: PROVIDER_QUOTA_INPUT,
        content_field: None,
        output: PROVIDER_QUOTA_OUTPUT,
        errors: READ_ERRORS,
        example: Example {
            input: "{}",
            output: r#"{"coordinator_generation":1,"cooldowns":[],"active_permits":[],"release_pending_permits":[]}"#,
        },
    },
    Command {
        resource: "model-profile",
        verb: "list",
        summary: "List organization model profiles",
        call: "model_profile.list",
        kind: Kind::List,
        input: &[],
        content_field: None,
        output: MODEL_PROFILE_FIELDS,
        errors: READ_ERRORS,
        example: Example {
            input: "{}",
            output: r#"[{"id":"0193a5c0-0000-7000-8000-000000000811","key":"default","name":"Default","description":"Primary profile","is_auto_generated":false,"archived":false,"connection_ids":["0193a5c0-0000-7000-8000-000000000801"],"revision":1}]"#,
        },
    },
];

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
