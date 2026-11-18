//! Native protected recovery client. No task/adoption or browser composition.
use crate::instance::setup::Paths;
use org_host_control::{ops, Password, SessionHandle};
use org_proto::{Call, Principal, Response};
use org_socket::rights::{ReceiveError, StreamReceiver};
use std::path::PathBuf;
use std::{
    io::Write,
    os::{fd::OwnedFd, unix::net::UnixStream},
    time::{Duration, Instant},
};
fn unavailable() -> String {
    "native operation is unresolved; retain original operation IDs and inspect persisted state"
        .into()
}
/// Exclusive byte/descriptor reader. No buffered ordinary reader can consume
/// SCM_RIGHTS. Fatal ancillary truncation requires process termination.
struct Wire {
    write: UnixStream,
    read: StreamReceiver,
    id: u64,
}
impl Wire {
    fn new(stream: UnixStream) -> Result<Self, String> {
        stream.set_nonblocking(false).map_err(|_| unavailable())?;
        stream
            .set_write_timeout(Some(Duration::from_secs(30)))
            .map_err(|_| unavailable())?;
        Ok(Self {
            read: StreamReceiver::new(stream.try_clone().map_err(|_| unavailable())?),
            write: stream,
            id: 0,
        })
    }

    fn byte_until(
        &mut self,
        count: usize,
        deadline: Instant,
    ) -> Result<(u8, Vec<OwnedFd>), String> {
        loop {
            match self.read.receive_byte(count, &crate::cloexec::SPAWN_LOCK) {
                Ok(value) => return Ok(value),
                Err(ReceiveError::Fatal(_)) => std::process::abort(),
                Err(ReceiveError::WouldBlock) => {
                    if Instant::now() >= deadline {
                        return Err(unavailable());
                    }
                    std::thread::sleep(Duration::from_millis(2));
                }
            }
        }
    }
    fn call<C: Call>(
        &mut self,
        params: &C::Params,
        principal: Option<&Principal>,
    ) -> Result<C::Result, String> {
        self.id = self
            .id
            .checked_add(1)
            .filter(|id| *id <= 9_007_199_254_740_991)
            .ok_or_else(unavailable)?;
        // Serialize typed parameters directly: avoid an extra secret-bearing
        // JSON Value tree, and wipe encoded password/session/grant bytes even
        // if a size check or write fails. Wire shape remains Request's shape.
        #[derive(serde::Serialize)]
        struct Outgoing<'a, P> {
            id: u64,
            method: &'static str,
            #[serde(skip_serializing_if = "Option::is_none")]
            principal: Option<&'a Principal>,
            params: &'a P,
        }
        struct Encoded(Vec<u8>);
        impl Drop for Encoded {
            fn drop(&mut self) {
                crate::secrets::zero(&mut self.0);
            }
        }
        let mut bytes = Encoded(Vec::new());
        serde_json::to_writer(
            &mut bytes.0,
            &Outgoing {
                id: self.id,
                method: C::METHOD,
                principal,
                params,
            },
        )
        .map_err(|_| unavailable())?;
        if bytes.0.len() > org_proto::MAX_FRAME_BYTES {
            return Err(unavailable());
        }
        bytes.0.push(b'\n');
        self.write
            .write_all(&bytes.0)
            .and_then(|_| self.write.flush())
            .map_err(|_| unavailable())?;
        drop(bytes);
        let mut raw = Encoded(Vec::new());
        let wait = if C::METHOD == ops::RecoveryRelease::METHOD {
            120
        } else {
            30
        };
        let deadline = Instant::now() + Duration::from_secs(wait);
        loop {
            if Instant::now() >= deadline {
                return Err(unavailable());
            }
            let (byte, empty) = self.byte_until(0, deadline)?;
            debug_assert!(empty.is_empty());
            if byte == b'\n' {
                break;
            }
            if raw.0.len() >= org_proto::MAX_FRAME_BYTES {
                return Err(unavailable());
            }
            raw.0.push(byte);
        }
        let response: Response = serde_json::from_slice(&raw.0).map_err(|_| unavailable())?;
        drop(raw);
        if response.id != self.id {
            return Err(unavailable());
        }
        response.into_result::<C>().map_err(|error| {
            #[cfg(test)]
            match &error {
                org_proto::CallError::Api(error) => {
                    eprintln!("native {} refused: {:?}", C::METHOD, error.code)
                }
                org_proto::CallError::Decode(_) => {
                    eprintln!("native {} result decode failed", C::METHOD)
                }
            }
            let _ = error;
            unavailable()
        })
    }
}
fn session(handle: &SessionHandle) -> SessionHandle {
    SessionHandle::new(handle.expose().into()).expect("live nonempty handle")
}

fn verify_host(stream: &UnixStream) -> Result<(), String> {
    if !org_socket::peer_allowed(stream, org_socket::effective_uid()) {
        return Err("host peer refused".into());
    }
    let token = match crate::peer::audit_token(stream) {
        Ok(token) => token,
        // A system that cannot identify the host's image has no identity to accept.
        Err(error) if crate::peer::unidentifiable(&error) => {
            return Err("host signing identity refused".into())
        }
        Err(_) => return Err("host identity unavailable".into()),
    };
    if let Some(team) = crate::APP_TEAM_ID {
        let req = crate::code_signing::developer_id_requirement(crate::HOST_SIGNING_ID, team)?;
        if crate::code_signing::satisfies_hardened(&token, &req) {
            return Ok(());
        }
    }
    #[cfg(debug_assertions)]
    if crate::peer::executable(stream).ok() == std::env::current_exe().ok() {
        return Ok(());
    }
    Err("host signing identity refused".into())
}
fn password() -> Result<Password, String> {
    terminal::password()
}
pub(super) fn paths(name: Option<&str>) -> Result<Paths, String> {
    if !cfg!(target_os = "macos") {
        return Err(
            "protected platform setup/custody is unavailable on this target; no HQ was provisioned"
                .into(),
        );
    }
    let account = crate::home::account_dir().ok_or("account home unavailable")?;
    Paths::prepare(&account, user_temp(&account)?, name).map_err(|_| unavailable())
}
fn user_temp(account: &std::path::Path) -> Result<PathBuf, String> {
    use org_platform::dirs::UserTempError;
    org_platform::dirs::user_temp_base(account).map_err(|error| match error {
        UserTempError::Unavailable => "OS private temporary directory unavailable".into(),
        UserTempError::Invalid(_) => unavailable(),
        UserTempError::Unsupported(_) => "protected platform setup is unavailable".into(),
    })
}
mod recovery;
pub use recovery::recovery;
mod terminal;
