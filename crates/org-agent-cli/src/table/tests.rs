//! The command table holds the org-command contract, and every write honours `--dry-run`.
use super::*;
use crate::transport::{OfficeTransport, TransportOffice};
use serde_json::{json, Value};

struct DryRunAuditTransport {
    calls: Vec<(String, Value)>,
}

impl OfficeTransport for DryRunAuditTransport {
    fn call_method(&mut self, method: &str, params: Value) -> Result<Value, org_command::Refusal> {
        self.calls.push((method.to_owned(), params));
        Ok(json!({ "status": "checked" }))
    }
}

#[test]
fn command_table_passes_all_org_command_contract_checks() {
    let table = table();
    org_command::testkit::check_rows(&table);
}

#[test]
fn the_runtime_table_holds_the_checked_rows_and_is_built_once() {
    let checked = table();
    let runtime = runtime_table();
    assert_eq!(runtime.rows(), checked.rows());
    assert!(std::ptr::eq(runtime, runtime_table()));
}

#[test]
fn every_write_command_honors_dry_run_without_live_side_effects() {
    let table = table();
    let mut checked = 0usize;
    for cmd in table.rows().iter().copied().filter(|c| c.is_write()) {
        checked += 1;
        let mut args = vec![cmd.resource.to_owned(), cmd.verb.to_owned()];
        if !cmd.input.is_empty() {
            args.push("--json".to_owned());
            args.push(cmd.example.input.to_owned());
        }
        args.push("--dry-run".to_owned());
        if matches!(
            cmd.kind,
            Kind::Write {
                if_match: IfMatch::Required,
                ..
            }
        ) {
            args.push("--if-match".to_owned());
            args.push("1".to_owned());
        }
        let mut office = TransportOffice(DryRunAuditTransport { calls: vec![] });
        let out = crate::execute_with_office("agent-os", &args, &mut office);
        let env: Value = serde_json::from_str(&out.stdout).unwrap_or_else(|e| {
            panic!(
                "command `{} {}` returned non-JSON stdout: {e}: {}",
                cmd.resource, cmd.verb, out.stdout
            )
        });
        assert_eq!(
            env["meta"]["dry_run"],
            Value::Bool(true),
            "`{} {} --dry-run` must stamp meta.dry_run=true: {}",
            cmd.resource,
            cmd.verb,
            out.stdout
        );
        if office.0.calls.is_empty() {
            assert_eq!(
                out.exit_code, 1,
                "`{} {} --dry-run` refused with unexpected exit code: {}",
                cmd.resource, cmd.verb, out.stdout
            );
            assert_eq!(env["ok"], Value::Bool(false));
            assert_eq!(
                env["error"]["code"], "unsupported",
                "`{} {} --dry-run` with zero RPC calls must fail closed with `unsupported`: {}",
                cmd.resource, cmd.verb, out.stdout
            );
        } else {
            for (method, params) in &office.0.calls {
                assert_eq!(
                    params.get("dry_run"),
                    Some(&Value::Bool(true)),
                    "`{} {} --dry-run` invoked `{method}` without dry_run=true! params={params}",
                    cmd.resource,
                    cmd.verb
                );
            }
            assert_eq!(
                out.exit_code, 0,
                "`{} {} --dry-run` failed despite forwarding dry_run=true: {}",
                cmd.resource, cmd.verb, out.stdout
            );
            assert_eq!(env["ok"], Value::Bool(true));
        }
    }
    assert!(
        checked >= 40,
        "expected at least 40 write commands, checked {checked}"
    );
}

#[test]
fn directory_card_presence_enum_covers_all_member_presence_variants() {
    let presence_field = DIRECTORY_CARD_FIELDS
        .iter()
        .find(|f| f.name == "presence")
        .expect("presence field in DIRECTORY_CARD_FIELDS");
    let allowed = match presence_field.ty {
        Type::Enum(values) => values,
        other => panic!("expected Type::Enum for presence, got {other:?}"),
    };
    let variants = [
        org_proto::directory::MemberPresence::Online,
        org_proto::directory::MemberPresence::Available,
        org_proto::directory::MemberPresence::Busy,
        org_proto::directory::MemberPresence::Idle,
        org_proto::directory::MemberPresence::NeedsYou,
        org_proto::directory::MemberPresence::WaitingForAnswer,
        org_proto::directory::MemberPresence::Paused,
        org_proto::directory::MemberPresence::UnableToRun,
        org_proto::directory::MemberPresence::WithYou,
        org_proto::directory::MemberPresence::Asleep,
        org_proto::directory::MemberPresence::Offline,
        org_proto::directory::MemberPresence::Unknown,
    ];
    assert_eq!(
        allowed.len(),
        variants.len(),
        "DIRECTORY_CARD_FIELDS presence enum must match MemberPresence variants 1:1"
    );
    for variant in variants {
        let wire = serde_json::to_value(variant).unwrap();
        let wire_str = wire.as_str().unwrap();
        assert!(
            allowed.contains(&wire_str),
            "DIRECTORY_CARD_FIELDS presence enum missing `{wire_str}`"
        );
    }
}
