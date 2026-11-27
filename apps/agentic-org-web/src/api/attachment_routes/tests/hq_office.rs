//! `TASK-98.5`: [`HqOffice`] bounds the whole call by its deadline, not only the office's
//! reply. Against a scripted host that stalls while the office channel is set up, the call
//! still ends at its deadline as a transport failure, long before the host answers.

use std::io;
use std::os::unix::net::{UnixDatagram, UnixStream};
use std::sync::Arc;
use std::time::{Duration, Instant};

use org_host_control::listen::{ListenMode, ListenOrigin};
use org_proto::attachments::{AttachmentInspect, AttachmentInspectParams};
use serde_json::{json, Value};

use super::super::office::{AttachmentOffice, HqOffice};
use crate::api::{CallerAuth, SessionApi};
use crate::host::HostControl;
use crate::office::OfficeFailure;
use crate::session::SessionCache;

const INSTANCE: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
const ORG: &str = "0193a5c0-0000-7000-8000-000000000001";
const FILE: &str = "0193a5c0-0000-7000-8000-0000000000bb";

/// The call's whole budget, and how long the host takes to answer a channel open.
const DEADLINE: Duration = Duration::from_millis(200);
const STALL: Duration = Duration::from_millis(1500);

/// The host's answer to `method`: a local session, one organization with its HQ office,
/// and a channel open that takes [`STALL`] before it fails.
fn answer(method: &str) -> Value {
    match method {
        "browser.local_session" => json!({"result": {
            "session_id": "S1", "account": {"id": "A1", "login": "alex"}, "admin": true,
            "orgs": [], "server_time_ms": 1_794_312_000_000u64,
            "idle_expires_at_ms": u64::MAX / 2,
        }}),
        "organization.list" => json!({"result": {"creating": [], "organizations": [{
            "id": ORG, "name": "Acme", "slug": "acme", "created_at": "2026-11-10T12:00:00.000Z",
            "employee_id": "0193a5c0-0000-7000-8000-000000000006", "recovery": "pending",
            "offices": [{"id": "0193a5c0-0000-7000-8000-000000000002", "organization_id": ORG,
                "name": "HQ", "role": "hq", "host": "This Mac", "status": "active",
                "endpoint": "/private/office.sock"}],
        }]}}),
        "browser.open_channel" => {
            std::thread::sleep(STALL);
            json!({"error": {"code": "unavailable", "message": "no"}})
        }
        _ => json!({"error": {"code": "unavailable", "message": "no"}}),
    }
}

/// The session API over a scripted host connection (no office is reachable).
fn scripted_api() -> Arc<SessionApi> {
    let (web, peer) = UnixStream::pair().unwrap();
    std::thread::spawn(move || {
        let mut reader = io::BufReader::new(peer);
        while let Ok(Some(request)) = org_proto::frame::read_message::<Value>(&mut reader) {
            let method = request["method"].as_str().unwrap_or_default();
            let mut frame = if method == "hello" {
                let hello = org_proto::HelloResult::host("test", "disposable");
                json!({"result": hello.with_instance_id(INSTANCE)})
            } else {
                answer(method)
            };
            frame["id"] = request["id"].clone();
            if org_proto::frame::write_message(reader.get_mut(), &frame).is_err() {
                return;
            }
        }
    });
    let host = HostControl::from_handoff(
        web.into(),
        UnixDatagram::pair().unwrap().0.into(),
        INSTANCE.into(),
        Arc::new(|| panic!("unexpected ancillary failure")),
        Arc::new(|_, _| {}),
    )
    .unwrap();
    let origin = ListenOrigin::parse("http://127.0.0.1:8080").unwrap();
    Arc::new(SessionApi::new(
        host,
        Arc::new(SessionCache::default()),
        &origin,
        ListenMode::HttpLoopback,
    ))
}

#[test]
fn a_stalled_channel_setup_ends_the_call_at_its_deadline() {
    let api = scripted_api();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let (reply, took) = runtime.block_on(async {
        let office = HqOffice::resolve(api, CallerAuth::LocalAuthless, ORG.to_owned())
            .await
            .ok()
            .expect("the HQ office resolves");
        let started = Instant::now();
        let params = AttachmentInspectParams {
            attachment_id: FILE.to_owned(),
        };
        let reply = office
            .call::<AttachmentInspect>(params, true, DEADLINE)
            .await;
        (reply, started.elapsed())
    });
    assert!(matches!(reply, Err(OfficeFailure::Transport)), "{reply:?}");
    assert!(took >= DEADLINE, "{took:?}");
    assert!(took < STALL, "the call waited for the host: {took:?}");
}
