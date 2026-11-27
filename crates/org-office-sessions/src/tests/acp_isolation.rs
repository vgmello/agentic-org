//! End-to-end live-process isolation tests for the unified ACP adapters (`gemini`, `kimi`, `pi`)
//! under [`SessionManager`], [`LocalOsSandbox`], and [`RunBindingRegistry`] (`TASK-271`).
//!
//! Proves that each ACP runtime receives ONLY its own enrolled vault credential (never another
//! agent's or runtime's credential), redacts session tokens and provider secrets in both replies
//! and discovered transcripts, respects `SandboxRunner` launch preparation & pre-spawn verification,
//! and enforces `RunBindingVerifier` peer/token checks fail-closed.

use super::*;
use crate::binding::{LocalPeerPidVerifier, PeerIdentity, RunBindingVerifier};
use crate::manager::{LocalOsSandbox, SandboxPrepareContext, SandboxRunner, SandboxVerifyContext};
use org_proto::sessions::{
    MemberSessionListParams, MemberSessionTranscriptParams, TranscriptFormat,
};

struct FakeAcpScript<'a> {
    vendor: &'a str,
    session_id: &'a str,
    model_id: &'a str,
    mode_id: &'a str,
    own_key_env: &'a str,
    foreign_env_a: &'a str,
    foreign_env_b: &'a str,
    transcript_dir_rel: &'a str,
}

fn fake_acp_script(spec: &FakeAcpScript<'_>) -> String {
    let FakeAcpScript {
        vendor,
        session_id,
        model_id,
        mode_id,
        own_key_env,
        foreign_env_a,
        foreign_env_b,
        transcript_dir_rel,
    } = spec;
    format!(
        r#"#!/bin/sh
while IFS= read -r line; do
  case "$line" in
    *'"method":"initialize"'*)
      printf '{{"jsonrpc":"2.0","id":1,"result":{{"protocolVersion":1,"agentInfo":{{"name":"{vendor}","version":"0.24.0"}}}}}}\n'
      ;;
    *'"method":"session/new"'*)
      printf '{{"jsonrpc":"2.0","id":2,"result":{{"sessionId":"{session_id}","models":{{"currentModelId":"{model_id}"}},"modes":{{"currentModeId":"{mode_id}"}}}}}}\n'
      ;;
    *'"method":"session/prompt"'*)
      mkdir -p "$HOME/{transcript_dir_rel}"
      eval "own_key=\${own_key_env}"
      eval "for_a=\${foreign_env_a}"
      eval "for_b=\${foreign_env_b}"
      printf '{{"role":"user","content":"Hello {vendor}"}}\n' > "$HOME/{transcript_dir_rel}/{session_id}.jsonl"
      printf '{{"role":"assistant","content":"Transcript leak key=%s tok=%s"}}\n' "$own_key" "$AGENT_OS_SESSION_TOKEN" >> "$HOME/{transcript_dir_rel}/{session_id}.jsonl"
      printf '{{"jsonrpc":"2.0","method":"session/update","params":{{"sessionId":"{session_id}","update":{{"sessionUpdate":"agent_message_chunk","content":{{"type":"text","text":"{vendor} reply tok=%s key=%s foreign=[%s|%s]"}}}}}}}}\n' "$AGENT_OS_SESSION_TOKEN" "$own_key" "$for_a" "$for_b"
      printf '{{"jsonrpc":"2.0","id":3,"result":{{"stopReason":"end_turn","usage":{{"inputTokens":24,"outputTokens":9}}}}}}\n'
      ;;
    *'"method":"session/close"'*)
      exit 0
      ;;
  esac
done
"#
    )
}

#[test]
fn gemini_kimi_and_pi_sessions_enforce_credential_isolation_redaction_and_transcript_discovery() {
    let _ = org_platform::process::harden_self();
    let office: Arc<Office> = Arc::new(Office::for_test(TestOffice::default()));
    let owner_id = office.owner_member_id().to_owned();
    let root_team_id = office.org_snapshot().unwrap().teams[0].id.clone();

    let (gemini_id, gemini_conn) = hire_agent_with_runtime(
        &office,
        &owner_id,
        &root_team_id,
        "google",
        "gemini",
        "gem-bot",
        "gemini-2.5-pro",
    );
    let (kimi_id, kimi_conn) = hire_agent_with_runtime(
        &office,
        &owner_id,
        &root_team_id,
        "moonshot",
        "kimi",
        "kimi-bot",
        "kimi-k2",
    );
    let (pi_id, pi_conn) = hire_agent_with_runtime(
        &office,
        &owner_id,
        &root_team_id,
        "pi",
        "pi",
        "pi-bot",
        "pi-sonnet",
    );

    let bin_dir = tempfile::tempdir().unwrap();
    let fake_gemini = write_fake_cli(
        bin_dir.path(),
        "gemini",
        &fake_acp_script(&FakeAcpScript {
            vendor: "gemini",
            session_id: "gemini-live-1",
            model_id: "gemini-2.5-pro",
            mode_id: "default",
            own_key_env: "GEMINI_API_KEY",
            foreign_env_a: "KIMI_API_KEY",
            foreign_env_b: "PI_API_KEY",
            transcript_dir_rel: ".gemini/sessions",
        }),
    );
    let fake_kimi = write_fake_cli(
        bin_dir.path(),
        "kimi",
        &fake_acp_script(&FakeAcpScript {
            vendor: "kimi",
            session_id: "kimi-live-1",
            model_id: "kimi-k2",
            mode_id: "default",
            own_key_env: "KIMI_API_KEY",
            foreign_env_a: "GEMINI_API_KEY",
            foreign_env_b: "PI_API_KEY",
            transcript_dir_rel: ".kimi/sessions",
        }),
    );
    let fake_pi = write_fake_cli(
        bin_dir.path(),
        "pi-acp",
        &fake_acp_script(&FakeAcpScript {
            vendor: "pi",
            session_id: "pi-live-1",
            model_id: "pi-sonnet",
            mode_id: "default",
            own_key_env: "PI_API_KEY",
            foreign_env_a: "GEMINI_API_KEY",
            foreign_env_b: "KIMI_API_KEY",
            transcript_dir_rel: ".pi/agent/sessions",
        }),
    );

    let mut registry = RuntimeRegistry::production();
    registry.register(
        Arc::new(org_agent_runtime_acp::GeminiRuntime::new()),
        Some(fake_gemini),
    );
    registry.register(
        Arc::new(org_agent_runtime_acp::KimiRuntime::new()),
        Some(fake_kimi),
    );
    registry.register(
        Arc::new(org_agent_runtime_acp::PiRuntime::new()),
        Some(fake_pi),
    );

    let sock_dir = tempfile::tempdir().unwrap();
    let mut cfg = SessionManagerConfig::production(sock_dir.path().join("office.sock"));
    cfg.runtimes = registry;
    cfg.helper_dir = Some(bin_dir.path().to_path_buf());
    let manager = SessionManager::new(Arc::clone(&office), cfg);

    let gemini_secret = "AIzaSyGeminiIsoSecret111111111";
    let kimi_secret = "sk-kimi-IsoSecret222222222222";
    let pi_secret = "sk-pi-IsoSecret33333333333333";

    for (conn_id, provider, secret) in [
        (&gemini_conn, "gemini", gemini_secret),
        (&kimi_conn, "kimi", kimi_secret),
        (&pi_conn, "pi", pi_secret),
    ] {
        let started = manager
            .enrollment()
            .start(&owner_id, conn_id, provider)
            .unwrap_or_else(|e| panic!("{provider} enroll start: {e}"));
        assert_eq!(started.state, EnrollmentState::AwaitingCode);
        let done = manager
            .enrollment()
            .submit_code(&owner_id, conn_id, &started.login_id, secret)
            .unwrap_or_else(|e| panic!("{provider} enroll submit: {e}"));
        assert_eq!(done.state, EnrollmentState::Completed);
    }

    let all_secrets = [gemini_secret, kimi_secret, pi_secret];
    for (vendor, agent_id, expected_sid, expected_fmt) in [
        (
            "gemini",
            &gemini_id,
            "gemini-live-1",
            TranscriptFormat::Gemini,
        ),
        ("kimi", &kimi_id, "kimi-live-1", TranscriptFormat::Kimi),
        ("pi", &pi_id, "pi-live-1", TranscriptFormat::Pi),
    ] {
        manager
            .start_session(
                &owner_id,
                &MemberSessionStartParams {
                    command_id: new_uuid_v7(),
                    employee_id: agent_id.clone(),
                    instruction: Some("Check isolation".into()),
                    workspace: None,
                },
            )
            .unwrap_or_else(|e| panic!("{vendor} start_session: {e:?}"));

        let reply = wait_for_reply(&manager, &owner_id, agent_id);
        let expected_reply = format!("{vendor} reply tok=[REDACTED] key=[REDACTED] foreign=[|]");
        assert!(
            reply.contains(&expected_reply),
            "{vendor}: unexpected reply (foreign credentials must be unset & own secrets redacted): {reply}"
        );
        for secret in all_secrets {
            assert!(
                !reply.contains(secret),
                "{vendor}: secret leaked in reply: {reply}"
            );
        }

        // Verify RunBindingVerifier refuses forged tokens and missing tokens for this agent.
        let verifier = LocalPeerPidVerifier::new(manager.bindings().clone());
        let peer = PeerIdentity::from_raw_pid(Some(std::process::id() as libc::pid_t));
        assert!(matches!(
            verifier.verify(agent_id, Some("forged-token"), &peer),
            Err(BindingRejection::UnknownToken)
        ));
        assert!(matches!(
            verifier.verify(agent_id, None, &peer),
            Err(BindingRejection::MissingToken)
        ));

        // Verify LocalOsSandbox prepares and verifies cleanly for the ACP adapter.
        let adapter = manager.runtimes().resolve(vendor).unwrap();
        let sandbox = LocalOsSandbox;
        let plan = sandbox
            .prepare_launch(&SandboxPrepareContext {
                office: &office,
                office_socket: &sock_dir.path().join("office.sock"),
                agent_os_state_roots: &[],
                employee_id: agent_id,
                sandbox_enabled: true,
                host_profile: adapter.host_profile(),
            })
            .unwrap();
        sandbox
            .verify_before_spawn(
                &SandboxVerifyContext {
                    office: &office,
                    employee_id: agent_id,
                    active_employees: &[],
                },
                &plan,
            )
            .unwrap();

        // Verify discovered transcript format + redaction.
        let list = manager
            .list_sessions(
                &owner_id,
                &MemberSessionListParams {
                    employee_id: Some(agent_id.clone()),
                    limit: Some(10),
                },
            )
            .expect("list_sessions");
        assert!(
            list.sessions
                .iter()
                .any(|s| s.session_id == expected_sid && s.format == expected_fmt),
            "{vendor}: expected {expected_sid} ({expected_fmt:?}) in {:?}",
            list.sessions
        );
        let tx = manager
            .read_session_transcript(
                &owner_id,
                &MemberSessionTranscriptParams {
                    employee_id: Some(agent_id.clone()),
                    session_id: Some(expected_sid.into()),
                    include_tool_calls: Some(true),
                    include_file_changes: Some(true),
                    include_subagents: Some(true),
                    cursor: None,
                    limit: Some(50),
                },
            )
            .expect("read_session_transcript");
        assert_eq!(tx.format, expected_fmt);
        let combined: String = tx
            .entries
            .iter()
            .filter_map(|e| e.text.as_deref())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            combined.contains("Transcript leak key=[REDACTED] tok=[REDACTED]"),
            "{vendor}: unexpected transcript text: {combined}"
        );
        for secret in all_secrets {
            assert!(
                !combined.contains(secret),
                "{vendor}: secret leaked in transcript: {combined}"
            );
        }

        let _ = manager.stop_session(agent_id, "done");
    }
}
