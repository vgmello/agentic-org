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
pub(crate) struct Wire {
    write: UnixStream,
    read: StreamReceiver,
    id: u64,
}
impl Wire {
    pub(crate) fn new(stream: UnixStream) -> Result<Self, String> {
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
    pub(crate) fn call<C: Call>(
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

pub(crate) fn verify_host(stream: &UnixStream) -> Result<(), String> {
    if !org_socket::peer_allowed(stream, org_socket::effective_uid()) {
        return Err("host peer refused".into());
    }
    #[cfg(target_os = "macos")]
    {
        let token = crate::peer::audit_token(stream).map_err(|_| "host identity unavailable")?;
        if let Some(team) = crate::APP_TEAM_ID {
            let req = crate::code_signing::developer_id_requirement(crate::HOST_SIGNING_ID, team)?;
            if crate::code_signing::satisfies_hardened(&token, &req) {
                return Ok(());
            }
        }
        #[cfg(debug_assertions)]
        if crate::peer::pid(stream)
            .and_then(crate::peer::executable)
            .ok()
            == std::env::current_exe().ok()
        {
            return Ok(());
        }
    }
    Err("host signing identity refused".into())
}
fn password() -> Result<Password, String> {
    terminal::password()
}
/// A development build may place every instance under an explicit application base,
/// `ORG_DEV_ACCOUNT_BASE` (absolute), with the user temp base `<base>/temp`: tests and dev runs
/// then never touch the account's real `Library` folders. A release build refuses the variable.
pub const DEV_ACCOUNT_BASE: &str = "ORG_DEV_ACCOUNT_BASE";

/// The instance paths of this account, for setup, `serve --name`, `admin` and `recovery`.
pub(crate) fn paths(name: Option<&str>) -> Result<Paths, String> {
    if let Some(base) = std::env::var_os(DEV_ACCOUNT_BASE).filter(|b| !b.is_empty()) {
        if !cfg!(debug_assertions) {
            return Err(format!(
                "{DEV_ACCOUNT_BASE} is a development setting; this release build refuses it"
            ));
        }
        let base = PathBuf::from(base);
        if !base.is_absolute() {
            return Err(format!("{DEV_ACCOUNT_BASE} must be an absolute path"));
        }
        let temp = base.join("temp");
        for dir in [&base, &temp] {
            match std::fs::create_dir(dir) {
                Ok(()) => {
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
                        .map_err(|e| e.to_string())?;
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(format!("creating {}: {e}", dir.display())),
            }
        }
        return Paths::at(&base, temp, name).map_err(|e| e.to_string());
    }
    if !cfg!(target_os = "macos") {
        return Err(
            "protected platform setup/custody is unavailable on this target; no HQ was provisioned"
                .into(),
        );
    }
    let account = crate::home::account_dir().ok_or("account home unavailable")?;
    Paths::prepare(&account, user_temp()?, name).map_err(|_| unavailable())
}
fn user_temp() -> Result<PathBuf, String> {
    #[cfg(target_os = "macos")]
    {
        let mut bytes = vec![0_u8; 4096];
        // SAFETY: module-owned writable storage and supported Darwin query.
        let n = unsafe {
            libc::confstr(
                libc::_CS_DARWIN_USER_TEMP_DIR,
                bytes.as_mut_ptr().cast(),
                bytes.len(),
            )
        };
        if n == 0 || n > bytes.len() || bytes[n - 1] != 0 {
            return Err("OS private temporary directory unavailable".into());
        }
        bytes.truncate(n - 1);
        let path = PathBuf::from(String::from_utf8(bytes).map_err(|_| unavailable())?);
        path.canonicalize().map_err(|_| unavailable())
    }
    #[cfg(not(target_os = "macos"))]
    Err("protected platform setup is unavailable".into())
}
mod recovery;
pub use recovery::recovery;
mod terminal;
