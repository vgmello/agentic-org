//! X-02 move 1: the additive v2 shapes and their vectors (`test-vectors/v2/proto-*.json`).

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::directory::DirectoryCard;
use crate::org::{MemberAddAgentParams, OrgChange};
use crate::retired::{self, APP_TOKEN_RETIRED, HOME_RETIRED};
use crate::work::{TaskCreateParams, WorkChangedData};
use crate::*;

fn vectors(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn range(v: &Value) -> ApiRange {
    serde_json::from_value(v.clone()).unwrap()
}

// ---- §3 negotiation ------------------------------------------------------------------------

/// What a connection does with each message, as X-02 §3 states it. The vectors are checked
/// against this, so a vector that contradicts the rules fails here, not in move 2a.
#[derive(Debug, PartialEq)]
enum Step {
    Answer(u32),
    Refuse,
    ProceedsAs(u32),
    /// `invalid_request`: the params didn't decode. Nothing is negotiated, and the connection
    /// stays open.
    Invalid,
}

fn oracle(accepts: ApiRange, v1_without_hello: bool, steps: &[Request]) -> Vec<Step> {
    let mut negotiated: Option<u32> = None;
    let mut out = Vec::new();
    for request in steps {
        let step = if request.is::<Hello>() {
            let Ok(params) = request.params::<Hello>() else {
                out.push(Step::Invalid);
                continue;
            };
            let offered = params.api_range();
            match negotiated {
                Some(n) if offered.contains(n) => Step::Answer(n),
                Some(_) => Step::Refuse,
                None => {
                    let low = accepts.min.max(offered.min);
                    let high = accepts.max.min(offered.max);
                    if low <= high {
                        negotiated = Some(high);
                        Step::Answer(high)
                    } else {
                        Step::Refuse
                    }
                }
            }
        } else {
            match negotiated {
                Some(n) => Step::ProceedsAs(n),
                None if v1_without_hello => {
                    negotiated = Some(1);
                    Step::ProceedsAs(1)
                }
                None => Step::Refuse,
            }
        };
        let refused = step == Step::Refuse;
        out.push(step);
        if refused {
            break; // the connection closes
        }
    }
    out
}

#[test]
fn negotiation_vectors_cover_every_cell_and_follow_the_rules() {
    let doc = vectors(include_str!("../test-vectors/v2/proto-hello.json"));
    let instance = doc["instance_id"].as_str().unwrap();
    let mut cells = std::collections::BTreeSet::new();
    for case in doc["cases"].as_array().unwrap() {
        let name = format!("{} / {}", case["connection"], case["name"]);
        let conn = &doc["connections"][case["connection"].as_str().unwrap()];
        let steps = case["steps"].as_array().unwrap();
        let requests: Vec<Request> = steps
            .iter()
            .map(|s| serde_json::from_value(s["send"].clone()).unwrap())
            .collect();
        let expected: Vec<Step> = steps
            .iter()
            .map(|s| {
                let e = &s["expect"];
                if let Some(result) = e.get("result") {
                    let hello: HelloResult = serde_json::from_value(result.clone()).unwrap();
                    if hello.api_version >= 2 {
                        assert_eq!(hello.instance_id.as_deref(), Some(instance), "{name}");
                    }
                    Step::Answer(hello.api_version)
                } else if let Some(code) = e.get("error") {
                    let code: ErrorCode = serde_json::from_value(code.clone()).unwrap();
                    if code == ErrorCode::InvalidRequest {
                        assert!(e.get("close").is_none(), "{name}");
                        return Step::Invalid;
                    }
                    assert_eq!(code, ErrorCode::UnsupportedVersion, "{name}");
                    assert_eq!(e["close"], true, "{name}");
                    Step::Refuse
                } else {
                    Step::ProceedsAs(e["proceeds_as"].as_u64().unwrap() as u32)
                }
            })
            .collect();
        assert_eq!(
            oracle(
                range(&conn["accepts"]),
                conn["v1_without_hello"].as_bool().unwrap(),
                &requests
            ),
            expected,
            "{name}"
        );
        let first = &requests[0];
        let column = if steps.len() > 1 && requests[1].is::<Hello>() {
            "repeated hello"
        } else if !first.is::<Hello>() {
            "first request not hello"
        } else if first
            .params::<Hello>()
            .is_ok_and(|params| params.api.is_none())
        {
            "v1 hello"
        } else {
            "hello {api}"
        };
        cells.insert((case["connection"].as_str().unwrap().to_owned(), column));
    }
    // Every cell of the §3 table: three connection phases, four columns.
    assert_eq!(cells.len(), 12, "{cells:?}");

    for case in doc["client_cases"].as_array().unwrap() {
        let offered = range(&case["offered"]);
        let hello: HelloResult = serde_json::from_value(case["result"].clone()).unwrap();
        let selected = case["selected_instance_id"].as_str().unwrap();
        let accept = offered.contains(hello.api_version)
            && (hello.api_version < 2 || hello.instance_id.as_deref() == Some(selected));
        let want = case["expect"].as_str().unwrap();
        assert_eq!(accept, want == "accept", "{}", case["name"]);
    }
}

#[test]
fn hello_params_default_to_v1_and_stay_compatible() {
    // A v1 hello is `{}`, both ways.
    assert_eq!(
        serde_json::to_value(HelloParams::default()).unwrap(),
        json!({})
    );
    let v1: HelloParams = serde_json::from_value(json!({})).unwrap();
    assert_eq!(v1.api_range(), ApiRange::V1);
    assert_eq!(ApiRange::V1, ApiRange { min: 1, max: 1 });
    let offer = HelloParams::offering(SUPPORTED_API, "org-gateway 0.1.0");
    assert_eq!(
        serde_json::to_value(&offer).unwrap(),
        json!({"api": {"min": 1, "max": 2}, "client": "org-gateway 0.1.0"})
    );
    assert_eq!(
        serde_json::from_value::<HelloParams>(json!({"api": {"min": 1, "max": 2}, "x": 1}))
            .unwrap()
            .api_range(),
        SUPPORTED_API
    );
    // The constants of move 1: v1 is still the reported version.
    assert_eq!(API_VERSION, 1);
    assert_eq!(SUPPORTED_API, ApiRange { min: 1, max: 2 });
    assert!(SUPPORTED_API.contains(1) && SUPPORTED_API.contains(2) && !SUPPORTED_API.contains(3));
    assert!(!ApiRange { min: 2, max: 1 }.contains(1));
    assert!(!ApiRange { min: 2, max: 1 }.contains(2));
    // A malformed range doesn't decode, so hello's params don't either (invalid_request).
    for bad in [
        json!({"min": 0, "max": 1}),
        json!({"min": 0, "max": 0}),
        json!({"min": 2, "max": 1}),
        json!({"min": 1}),
        json!({"min": "1", "max": 2}),
        json!({"min": -1, "max": 2}),
        json!({"min": 1.5, "max": 2}),
        json!({"min": 1, "max": 2, "x": 0}),
        json!({"min": 1, "max": 4_294_967_296u64}),
        json!([1, 2]),
        json!(2),
    ] {
        assert!(
            serde_json::from_value::<ApiRange>(bad.clone()).is_err(),
            "{bad}"
        );
        assert!(serde_json::from_value::<HelloParams>(json!({ "api": bad })).is_err());
    }
    // `"api": null` is an absent api: a v1 hello. A null range on its own isn't a range.
    assert!(serde_json::from_value::<ApiRange>(json!(null)).is_err());
    let null_api: HelloParams = serde_json::from_value(json!({"api": null})).unwrap();
    assert_eq!(null_api.api_range(), ApiRange::V1);
    // A non-string client, null included, is malformed too; an absent field is still v1.
    for bad in [json!({"client": 7}), json!({"client": null})] {
        assert!(
            serde_json::from_value::<HelloParams>(bad.clone()).is_err(),
            "{bad}"
        );
    }
    for good in [
        json!({"min": 1, "max": 1}),
        json!({"min": 2, "max": 5}),
        json!({"min": 1, "max": u32::MAX}),
    ] {
        let range: ApiRange = serde_json::from_value(good.clone()).unwrap();
        assert_eq!(serde_json::to_value(range).unwrap(), good);
    }
}

#[test]
fn hello_result_carries_instance_id_only_when_set() {
    let v1 = HelloResult::host("0.0.0", "This Mac");
    assert!(serde_json::to_value(&v1)
        .unwrap()
        .get("instance_id")
        .is_none());
    let v2 = HelloResult::host("0.0.0", "This Mac").with_instance_id("01M72N8MG00000000000000I01");
    let wire = serde_json::to_value(&v2).unwrap();
    assert_eq!(wire["instance_id"], "01M72N8MG00000000000000I01");
    assert_eq!(serde_json::from_value::<HelloResult>(wire).unwrap(), v2);
}

// ---- §9 error codes ------------------------------------------------------------------------

#[test]
fn the_two_new_codes_have_their_wire_names() {
    for (code, wire) in [
        (ErrorCode::UnsupportedVersion, "unsupported_version"),
        (ErrorCode::OperationReused, "operation_reused"),
    ] {
        assert_eq!(serde_json::to_value(code).unwrap(), wire);
        assert_eq!(
            serde_json::from_value::<ErrorCode>(json!(wire)).unwrap(),
            code
        );
    }
    // The code and the office status reason share a wire name but not a type.
    assert_eq!(
        serde_json::to_value(OfficeStatusReason::UnsupportedVersion).unwrap(),
        "unsupported_version"
    );
}

// ---- §4.1 grants and principals ------------------------------------------------------------

fn grant_vectors() -> Value {
    vectors(include_str!("../test-vectors/v2/proto-grant-mac.json"))
}

fn mac_of(hex_text: &str) -> GrantMac {
    serde_json::from_value(json!(hex_text)).unwrap()
}

#[test]
fn grant_mac_input_matches_the_vectors() {
    let doc = grant_vectors();
    let mut inputs = std::collections::BTreeSet::new();
    for case in doc["cases"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let grant: Grant = serde_json::from_value(case["grant"].clone()).unwrap();
        let input = grant_mac_input(&grant.claims).unwrap();
        assert_eq!(hex(&input), case["mac_input_hex"], "{name}");
        assert!(input.starts_with(b"org-grant/1\0"), "{name}");
        assert_eq!(
            grant.mac,
            mac_of(case["mac_hex"].as_str().unwrap()),
            "{name}"
        );
        // Written back exactly, fields in the contract's order.
        assert_eq!(
            serde_json::to_value(&grant).unwrap(),
            case["grant"],
            "{name}"
        );
        let text = serde_json::to_string(&grant).unwrap();
        let order = [
            "\"account\"",
            "\"member\"",
            "\"org_id\"",
            "\"office_id\"",
            "\"channel_id\"",
            "\"session_id\"",
            "\"exp\"",
            "\"mac\"",
        ]
        .map(|key| text.find(key).unwrap());
        assert!(order.windows(2).all(|w| w[0] < w[1]), "{name}: {text}");
        assert!(inputs.insert(case["mac_input_hex"].as_str().unwrap().to_owned()));
    }
    // Every case, the field-boundary pair included, has its own input.
    assert_eq!(inputs.len(), doc["cases"].as_array().unwrap().len());
}

#[test]
fn grant_mac_input_is_length_prefixed_big_endian() {
    let claims = GrantClaims {
        account: "a".into(),
        member: "".into(),
        org_id: "o".into(),
        office_id: "f".into(),
        channel_id: "c".into(),
        session_id: "s".into(),
        exp: 0x0102_0304_0506_0708,
    };
    let mut want = b"org-grant/1\0".to_vec();
    for field in ["a", "", "o", "f", "c", "s"] {
        want.extend_from_slice(&(field.len() as u32).to_be_bytes());
        want.extend_from_slice(field.as_bytes());
    }
    want.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
    assert_eq!(grant_mac_input(&claims).unwrap(), want);
    assert_eq!(GRANT_MAC_DOMAIN, "org-grant/1");
}

#[test]
fn a_request_carries_its_principal_and_hides_the_mac() {
    let doc = grant_vectors();
    let wire = &doc["request"]["wire"];
    let mac = doc["cases"][0]["mac_hex"].as_str().unwrap();
    let request: Request = serde_json::from_value(wire.clone()).unwrap();
    let principal = request.principal().unwrap().unwrap();
    assert_eq!(
        principal.grant.claims.channel_id,
        "01M72N8MG00000000000000C01"
    );
    assert_eq!(principal.grant.mac, mac_of(mac));
    assert_eq!(serde_json::to_value(&request).unwrap(), *wire);

    // Built from the typed call, it writes the same envelope, `principal` before `params`.
    let params: org::TeamCreateParams = request.params::<org::TeamCreate>().unwrap();
    let built = Request::new::<org::TeamCreate>(42, &params)
        .unwrap()
        .with_principal(&principal)
        .unwrap();
    assert_eq!(built, request);
    let text = serde_json::to_string(&built).unwrap();
    assert!(text.starts_with(r#"{"id":42,"method":"team.create","principal":{"grant":{"#));
    assert!(text.find("\"params\"").unwrap() > text.find("\"principal\"").unwrap());

    // Debug never shows the MAC: not the request's, the principal's or the grant's.
    for printed in [
        format!("{request:?}"),
        format!("{principal:?}"),
        format!("{:?}", principal.grant),
    ] {
        assert!(!printed.contains(mac), "{printed}");
    }
    assert!(format!("{request:?}").contains("principal: <elided"));
    assert_eq!(format!("{:?}", principal.grant.mac), "GrantMac(<redacted>)");

    // No principal: absent on the wire, `None` when read, and absent from Debug.
    let plain = Request::new::<Hello>(1, &HelloParams::default()).unwrap();
    assert!(serde_json::to_value(&plain)
        .unwrap()
        .get("principal")
        .is_none());
    assert!(plain.principal().unwrap().is_none());
    assert!(!format!("{plain:?}").contains("principal"));
}

#[test]
fn a_malformed_principal_fails_alone_and_never_quotes_the_mac() {
    let doc = grant_vectors();
    let mac = doc["cases"][0]["mac_hex"].as_str().unwrap();
    for case in doc["refused_principals"]["cases"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let wire = json!({
            "id": 7, "method": "who", "principal": case["principal"], "params": {}
        });
        // The envelope still decodes, so the server can answer this request.
        let request: Request = serde_json::from_value(wire).unwrap();
        assert!(request.principal.is_some(), "{name}");
        assert_eq!(
            request.principal().unwrap_err(),
            MalformedPrincipal,
            "{name}"
        );
        // Decoded directly, as a host-control result is, the error doesn't quote it either.
        let error = serde_json::from_value::<Principal>(case["principal"].clone())
            .unwrap_err()
            .to_string();
        assert!(
            !error.to_lowercase().contains(&mac[..16]),
            "{name}: {error}"
        );
        let error = serde_json::from_str::<Principal>(&case["principal"].to_string())
            .unwrap_err()
            .to_string();
        assert!(
            !error.to_lowercase().contains(&mac[..16]),
            "{name}: {error}"
        );
    }
    // The vectors include the shapes that would put the MAC in serde's own message.
    assert!(doc["refused_principals"]["cases"]
        .as_array()
        .unwrap()
        .iter()
        .any(|c| c["principal"] == json!(mac)));
}

#[test]
fn grant_secrets_redact_refuse_quietly_and_compare_by_value() {
    let mac = GrantMac::from_bytes([0x5a; GRANT_MAC_BYTES]);
    assert_eq!(format!("{mac:?}"), "GrantMac(<redacted>)");
    assert_eq!(serde_json::to_value(&mac).unwrap(), json!("5a".repeat(32)));
    assert_eq!(mac, mac_of(&"5a".repeat(32)));
    assert_ne!(mac, mac_of(&"5b".repeat(32)));
    let key = GrantKey::from_bytes([0xc3; GRANT_KEY_BYTES]);
    assert_eq!(format!("{key:?}"), "GrantKey(<redacted>)");
    assert_eq!(key.expose_bytes(), &[0xc3; GRANT_KEY_BYTES]);
    let back: GrantKey = serde_json::from_value(json!("c3".repeat(32))).unwrap();
    assert_eq!(back, key);
    for bad in [
        json!("C3".repeat(32)),
        json!("c3".repeat(31)),
        json!(1234),
        json!(null),
    ] {
        let error = serde_json::from_value::<GrantKey>(bad.clone())
            .unwrap_err()
            .to_string();
        assert!(
            !error.contains("c3c3") && !error.contains("C3C3"),
            "{error}"
        );
        let error = serde_json::from_value::<GrantMac>(bad)
            .unwrap_err()
            .to_string();
        assert!(
            !error.contains("c3c3") && !error.contains("C3C3"),
            "{error}"
        );
    }
}

// ---- §4.2 fingerprints and AgentProfileInput -----------------------------------------------

/// `member.add_agent`'s params as move 2a will type them: the same fields, with the profile as
/// [`AgentProfileInput`].
#[derive(Debug, Serialize, Deserialize)]
struct MemberAddAgentParamsV2 {
    command_id: String,
    member_id: String,
    name: String,
    office_id: String,
    profile: AgentProfileInput,
}

fn fingerprint_vectors() -> Value {
    vectors(include_str!("../test-vectors/v2/proto-fingerprints.json"))
}

#[test]
fn agent_profile_input_serializes_byte_identically_to_the_v1_profile() {
    let doc = fingerprint_vectors();
    for case in doc["org"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let serialized = case["serialized"].as_str().unwrap();
        let v1: MemberAddAgentParams = serde_json::from_value(case["params"].clone()).unwrap();
        let v2: MemberAddAgentParamsV2 = serde_json::from_value(case["params"].clone()).unwrap();
        assert_eq!(serde_json::to_string(&v1).unwrap(), serialized, "{name}");
        assert_eq!(serde_json::to_string(&v2).unwrap(), serialized, "{name}");
        // And the profile on its own, both ways, as stored.
        assert_eq!(
            serde_json::to_string(&v1.profile).unwrap(),
            serde_json::to_string(&v2.profile).unwrap(),
            "{name}"
        );
        assert_eq!(AgentProfile::from(v2.profile), v1.profile, "{name}");
        assert_eq!(case["fingerprint"].as_str().unwrap().len(), 64, "{name}");
    }

    // A v1 command stored with a home keeps its v1 serialization, but v2 refuses the input.
    let stored = &doc["org_refused_v1_home"];
    let v1: MemberAddAgentParams = serde_json::from_value(stored["params"].clone()).unwrap();
    assert_eq!(serde_json::to_string(&v1).unwrap(), stored["serialized"]);
    let error = serde_json::from_value::<MemberAddAgentParamsV2>(stored["params"].clone())
        .unwrap_err()
        .to_string();
    assert!(
        error.contains(stored["v2_refusal"].as_str().unwrap()),
        "{error}"
    );

    // The work vector's params are real `task.create` params, re-encoded unchanged.
    for case in doc["work"].as_array().unwrap() {
        let typed: TaskCreateParams = serde_json::from_value(case["params"].clone()).unwrap();
        assert_eq!(serde_json::to_value(&typed).unwrap(), case["params"]);
    }
}

// ---- §4.3 retired fields -------------------------------------------------------------------

/// The retired fields of `member.update_agent` and `msg.bind`, as move 2a will declare them on
/// those params. Other fields are ignored, as every params type does.
#[derive(Debug, Deserialize)]
struct UpdateAgentRetired {
    #[serde(default, deserialize_with = "retired::home")]
    #[allow(dead_code)]
    home: Option<Retired>,
    #[serde(default, deserialize_with = "retired::home")]
    #[allow(dead_code)]
    clear_home: Option<Retired>,
}

#[derive(Debug, Deserialize)]
struct BindRetired {
    #[serde(default, deserialize_with = "retired::app_token")]
    #[allow(dead_code)]
    app_token: Option<Retired>,
}

fn decode_retired(method: &str, params: &Value) -> Result<(), String> {
    let result = match method {
        "member.update_agent" => serde_json::from_value::<UpdateAgentRetired>(params.clone())
            .map(drop)
            .map_err(|e| e.to_string()),
        "member.add_agent" => serde_json::from_value::<MemberAddAgentParamsV2>(params.clone())
            .map(drop)
            .map_err(|e| e.to_string()),
        "msg.bind" => serde_json::from_value::<BindRetired>(params.clone())
            .map(drop)
            .map_err(|e| e.to_string()),
        other => panic!("no retired fields on {other}"),
    };
    result
}

#[test]
fn retired_fields_are_refused_on_presence_null_included() {
    let doc = vectors(include_str!("../test-vectors/v2/proto-retired-fields.json"));
    assert_eq!(doc["code"], "invalid_request");
    for case in doc["cases"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let error =
            decode_retired(case["method"].as_str().unwrap(), &case["params"]).expect_err(name);
        assert!(
            error.contains(case["message"].as_str().unwrap()),
            "{name}: {error}"
        );
        // The refused value never appears in the message.
        assert!(
            !error.contains("abab") && !error.contains("/Users/ada"),
            "{name}: {error}"
        );
        // Move 1 changes no behaviour: the v1 params type still reads the field, until move 2a
        // declares it retired on the params themselves.
        if case["method"] == "member.add_agent" {
            let request = Request {
                id: 1,
                method: "member.add_agent".into(),
                principal: None,
                params: case["params"].clone(),
            };
            assert!(request.params::<org::MemberAddAgent>().is_ok(), "{name}");
        }
    }
    for case in doc["accepted"].as_array().unwrap() {
        decode_retired(case["method"].as_str().unwrap(), &case["params"])
            .unwrap_or_else(|e| panic!("{}: {e}", case["name"]));
    }
    assert_eq!(HOME_RETIRED, "home is managed by the office");
    assert_eq!(APP_TOKEN_RETIRED, "app_token is retired");
}

#[test]
fn agent_profile_input_never_writes_home() {
    let input: AgentProfileInput = serde_json::from_value(json!({
        "role": "r", "title": "t", "runtime": "codex", "messaging_name": "m", "status": "active"
    }))
    .unwrap();
    assert!(input.home.is_none());
    let wire = serde_json::to_value(&input).unwrap();
    assert!(wire.get("home").is_none());
    assert_eq!(
        wire,
        json!({"role": "r", "title": "t", "runtime": "codex", "messaging_name": "m", "status": "active"})
    );
}

// ---- §8 managed home, actor, operation.get -------------------------------------------------

#[test]
fn managed_home_is_reported_on_the_card_only_when_set() {
    let home = ManagedHome {
        path:
            "/Users/ada/.agentic-org/agents/01M72N8MG00000000000000F01/01M72N8MG00000000000000M10/"
                .into(),
        office_id: "01M72N8MG00000000000000F01".into(),
        observed_at: "2026-11-10T12:00:00.000Z".into(),
    };
    let wire = json!({
        "path": "/Users/ada/.agentic-org/agents/01M72N8MG00000000000000F01/01M72N8MG00000000000000M10/",
        "office_id": "01M72N8MG00000000000000F01",
        "observed_at": "2026-11-10T12:00:00.000Z"
    });
    assert_eq!(serde_json::to_value(&home).unwrap(), wire);
    let card = json!({
        "id": "01M72N8MG00000000000000M10", "name": "Ada", "kind": "agent",
        "presence": "online", "teams": []
    });
    let old: DirectoryCard = serde_json::from_value(card.clone()).unwrap();
    assert!(old.managed_home.is_none());
    assert!(serde_json::to_value(&old)
        .unwrap()
        .get("managed_home")
        .is_none());
    let mut with_home = card;
    with_home["managed_home"] = wire;
    let new: DirectoryCard = serde_json::from_value(with_home.clone()).unwrap();
    assert_eq!(new.managed_home, Some(home));
    assert_eq!(serde_json::to_value(&new).unwrap(), with_home);
}

#[test]
fn changes_name_their_actor_when_known() {
    let actor = Actor {
        member: "01M72N8MG00000000000000M01".into(),
        kind: ActorKind::Human,
    };
    let org = json!({
        "position": {"epoch": 1, "seq": 4},
        "command_id": "01M72N8MG00000000000000P01",
        "changes": [{"type": "team_deleted", "team_id": "t"}]
    });
    let old: org::OrgChangedData = serde_json::from_value(org.clone()).unwrap();
    assert!(old.actor.is_none());
    assert_eq!(serde_json::to_value(&old).unwrap(), org);
    let mut new = org;
    new["actor"] = json!({"member": "01M72N8MG00000000000000M01", "kind": "human"});
    let decoded: org::OrgChangedData = serde_json::from_value(new.clone()).unwrap();
    assert_eq!(decoded.actor.as_ref(), Some(&actor));
    assert!(matches!(decoded.changes[0], OrgChange::TeamDeleted { .. }));
    assert_eq!(serde_json::to_value(&decoded).unwrap(), new);

    let work =
        json!({"position": {"incarnation": 1, "seq": 2}, "part": 1, "parts": 1, "changes": []});
    let old: WorkChangedData = serde_json::from_value(work.clone()).unwrap();
    assert!(old.actor.is_none());
    let mut new = work;
    new["actor"] = json!({"member": "01M72N8MG00000000000000M02", "kind": "agent"});
    let decoded: WorkChangedData = serde_json::from_value(new.clone()).unwrap();
    assert_eq!(decoded.actor.unwrap().kind, ActorKind::Agent);

    let unknown: Actor = serde_json::from_value(json!({"member": "m", "kind": "system"})).unwrap();
    assert_eq!(unknown.kind, ActorKind::Unknown);
    // It is never sent: writing it fails rather than inventing a kind.
    assert!(serde_json::to_value(ActorKind::Unknown).is_err());
    assert!(serde_json::to_value(&unknown).is_err());
}

#[test]
fn operation_get_has_its_wire_shapes() {
    assert_eq!(OperationGet::METHOD, "operation.get");
    let params = OperationGetParams {
        kind: OperationKind::Work,
        id: "01M72N8MG00000000000000W01".into(),
    };
    assert_eq!(
        serde_json::to_value(&params).unwrap(),
        json!({"kind": "work", "id": "01M72N8MG00000000000000W01"})
    );
    for (kind, wire) in [
        (OperationKind::Org, "org"),
        (OperationKind::Work, "work"),
        (OperationKind::Msg, "msg"),
    ] {
        assert_eq!(serde_json::to_value(kind).unwrap(), wire);
    }
    // The kind is required and must be known: a server answers anything else invalid_request.
    for bad in [json!({"id": "x"}), json!({"kind": "host", "id": "x"})] {
        assert!(serde_json::from_value::<OperationGetParams>(bad).is_err());
    }

    let committed = json!({"status": "committed", "result": {"status": "committed", "position": {"epoch": 1, "seq": 7}}});
    let outcome: OperationOutcome = serde_json::from_value(committed.clone()).unwrap();
    let OperationOutcome::Committed { result } = &outcome else {
        panic!("{outcome:?}");
    };
    let accepted: CommandAccepted = serde_json::from_value(result.clone()).unwrap();
    assert_eq!(accepted.position.unwrap().seq, 7);
    assert_eq!(serde_json::to_value(&outcome).unwrap(), committed);

    let rejected =
        json!({"status": "rejected", "refusal": {"code": "already_exists", "message": "Taken."}});
    let outcome: OperationOutcome = serde_json::from_value(rejected.clone()).unwrap();
    assert_eq!(
        outcome,
        OperationOutcome::Rejected {
            refusal: ApiError::new(ErrorCode::AlreadyExists, "Taken.")
        }
    );
    assert_eq!(serde_json::to_value(&outcome).unwrap(), rejected);

    // `queued` arrives with relay (X-05): an older reader sees Unknown and looks again later.
    let queued: OperationOutcome = serde_json::from_value(json!({"status": "queued"})).unwrap();
    assert_eq!(queued, OperationOutcome::Unknown);
    // It is never sent: writing it fails rather than inventing a status.
    assert!(serde_json::to_value(&queued).is_err());
    let response = Response::ok::<OperationGet>(3, &outcome).unwrap();
    assert!(response.into_result::<OperationGet>().is_ok());
}
