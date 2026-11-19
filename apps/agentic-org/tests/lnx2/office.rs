//! The office the LNX-2 container test's worker serves (`deploy/container/lnx2.sh`).
//!
//! No command creates an office yet without a signed-in administrator, so the test stores
//! one as the host's provisioning leaves it, exactly as the LNX-1 test does in its own
//! process (`tests/lnx1.rs`): an HQ office with two agents, made with the office library,
//! and its office secrets key in the office keys' item store of the instance's credential
//! directory (`/secrets/office-keys`), which every build of the image keeps its keys in. The
//! test copies this program into the container and runs it there as the service user while
//! the host is stopped; the host finds the office at its next start and starts the worker.
//!
//! ```text
//! lnx2-office <office home> <the office keys' item store>
//! ```
//!
//! It refuses an office home that already has the office, so a second run changes nothing.
use std::fs;
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::PathBuf;
use std::process::ExitCode;

use org_office::rules::OrgCommand;
use org_proto::org::{AgentProfile, AgentRuntime, AgentStatus, MemberAddAgentParams};
use org_proto::OfficeSecretsKey;

const OFFICE: &str = "01OFFICE00000000000000HQ1";
const ORG: &str = "01ORG0000000000000000ACME1";
const AGENTS: [(u8, &str, &str); 2] = [
    (1, "AGENT00000000000000000PIXL", "pixel"),
    (2, "AGENT00000000000000000NOVA", "nova"),
];
/// The office's secrets key: the office keys' item store holds its 32 bytes.
const KEY: [u8; 32] = [0xB3; 32];

/// A ULID made now (the command ids the office accepts), distinct for each `n`.
fn command_id(n: u8) -> String {
    const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |now| now.as_millis());
    let mut value = (ms << 80) | u128::from(n);
    let mut out = [0u8; 26];
    for slot in out.iter_mut().rev() {
        *slot = CROCKFORD[(value & 0x1F) as usize];
        value >>= 5;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn store(office_home: PathBuf, keys: PathBuf) -> Result<(), String> {
    let folder = org_office_launch::office_dir(&office_home, OFFICE)
        .ok_or("the office home has no folder for this office")?;
    if folder.exists() {
        return Err(format!("{} already exists", folder.display()));
    }
    let office = org_office::Office::create(
        folder,
        &org_office::NewOffice {
            office_id: OFFICE.into(),
            office_name: "HQ".into(),
            role: org_office::OfficeRole::Hq,
            org_id: ORG.into(),
            org_name: "Acme".into(),
        },
        org_office::Key::from_office_secrets_key(OfficeSecretsKey::from_bytes(KEY)),
    )
    .map_err(|e| format!("creating the office: {e:?}"))?;
    for (n, id, handle) in AGENTS {
        office
            .submit_org(OrgCommand::MemberAddAgent(MemberAddAgentParams {
                command_id: command_id(n),
                member_id: id.into(),
                name: handle.to_uppercase(),
                office_id: OFFICE.into(),
                profile: AgentProfile {
                    role: "engineer".into(),
                    title: "Engineer".into(),
                    focus: None,
                    runtime: AgentRuntime::ClaudeCode,
                    model: None,
                    run_mode: None,
                    schedule: None,
                    home: None,
                    messaging_name: handle.into(),
                    status: AgentStatus::Active,
                    offboarded_at: None,
                },
            }))
            .map_err(|e| format!("hiring {handle}: {e:?}"))?
            .map_err(|e| format!("hiring {handle}: {e:?}"))?;
    }
    drop(office);
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&keys)
        .map_err(|e| format!("{}: {e}", keys.display()))?;
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(keys.join(format!("{OFFICE}.key")))
        .and_then(|mut file| file.write_all(&KEY))
        .map_err(|e| format!("storing the office's key: {e}"))
}

fn main() -> ExitCode {
    let arguments: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    let [office_home, keys] = arguments.as_slice() else {
        eprintln!("usage: lnx2-office <office home> <the office keys' item store>");
        return ExitCode::from(2);
    };
    match store(office_home.clone(), keys.clone()) {
        Ok(()) => {
            println!("{OFFICE}");
            ExitCode::SUCCESS
        }
        Err(why) => {
            eprintln!("lnx2-office: {why}");
            ExitCode::FAILURE
        }
    }
}
