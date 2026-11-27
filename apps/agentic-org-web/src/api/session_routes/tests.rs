use super::*;

#[test]
fn transcript_query_and_session_id_validation() {
    let parsed = parse_transcript_query(Some(
        "include_tool_calls=false&include_file_changes=0&include_subagents=true&cursor=25&limit=100",
    ))
    .ok()
    .expect("valid transcript query");
    assert_eq!(parsed.include_tool_calls, Some(false));
    assert_eq!(parsed.include_file_changes, Some(false));
    assert_eq!(parsed.include_subagents, Some(true));
    assert_eq!(parsed.cursor, Some(25));
    assert_eq!(parsed.limit, Some(100));

    assert!(parse_transcript_query(Some("unknown=1")).is_err());
    assert!(parse_transcript_query(Some("limit=0")).is_err());
    assert!(parse_transcript_query(Some("limit=5&limit=10")).is_err());
    assert!(parse_transcript_query(Some("include_tool_calls=maybe")).is_err());
    assert_eq!(parse_list_query(Some("limit=20")).ok(), Some(Some(20)));
    assert!(parse_list_query(Some("limit=0")).is_err());
    assert!(parse_list_query(Some("limit=1&limit=2")).is_err());

    assert!(valid_session_id("sess-0193a5c0_1.v1").is_ok());
    assert!(valid_session_id("..").is_err());
    assert!(valid_session_id("../etc").is_err());
    assert!(valid_session_id("a..b").is_err());
    assert!(valid_session_id("").is_err());
}

#[test]
fn turn_control_bodies_become_interrupt_and_steer_params_for_the_named_agent() {
    let op = "01a1c554-5200-7000-8000-000000000006";
    let agent = "0193a5c0-0000-7000-8000-000000000050";
    let route = |build: fn(CallerAuth, String, String) -> SessionRoute| {
        build(CallerAuth::LocalAuthless, "org".into(), agent.into())
    };
    let interrupt = calls::prepare(
        route(SessionRoute::Interrupt),
        format!(r#"{{"command_id":"{op}"}}"#).as_bytes(),
    );
    assert!(interrupt.is_ok());
    let steer = calls::prepare(
        route(SessionRoute::Steer),
        format!(r#"{{"command_id":"{op}","text":"Use staging"}}"#).as_bytes(),
    );
    assert!(steer.is_ok());
    for bad in [
        format!(r#"{{"command_id":"{op}","text":""}}"#),
        format!(r#"{{"command_id":"{op}","text":"\n\t "}}"#),
        format!(r#"{{"command_id":"{op}"}}"#),
        r#"{"text":"t"}"#.to_owned(),
    ] {
        assert!(
            calls::prepare(route(SessionRoute::Steer), bad.as_bytes()).is_err(),
            "{bad}"
        );
    }
    assert!(is_known_agent_action("interrupt") && is_known_agent_action("steer"));
}
