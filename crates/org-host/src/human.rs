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
    pub(crate) fn receive_office_channel(&mut self) -> Result<UnixStream, String> {
        let deadline = Instant::now() + Duration::from_secs(30);
        let (marker, mut fds) = self.byte_until(1, deadline)?;
        if marker != ops::HOST_SOCK_MARKER || fds.len() != 1 {
            return Err(unavailable());
        }
        let stream = org_socket::rights::connected_unix_stream(fds.pop().unwrap())
            .map_err(|_| unavailable())?;
        stream.set_nonblocking(false).map_err(|_| unavailable())?;
        stream
            .set_read_timeout(Some(Duration::from_secs(30)))
            .map_err(|_| unavailable())?;
        stream
            .set_write_timeout(Some(Duration::from_secs(30)))
            .map_err(|_| unavailable())?;
        Ok(stream)
    }

    pub(crate) fn call<C: Call>(
        &mut self,
        params: &C::Params,
        principal: Option<&Principal>,
    ) -> Result<C::Result, String> {
        self.call_answer::<C>(params, principal)?
            .map_err(|_| unavailable())
    }

    /// [`Wire::call`], with the code of a refusal the host answered kept: `Ok(Err(code))`.
    /// Only its code: an error's message is the host's and is not shown.
    pub(crate) fn call_answer<C: Call>(
        &mut self,
        params: &C::Params,
        principal: Option<&Principal>,
    ) -> Result<Result<C::Result, org_proto::ErrorCode>, String> {
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
        match response.into_result::<C>() {
            Ok(result) => Ok(Ok(result)),
            Err(org_proto::CallError::Api(error)) => {
                #[cfg(test)]
                eprintln!("native {} refused: {:?}", C::METHOD, error.code);
                Ok(Err(error.code))
            }
            Err(org_proto::CallError::Decode(_)) => {
                #[cfg(test)]
                eprintln!("native {} result decode failed", C::METHOD);
                Err(unavailable())
            }
        }
    }
}
fn session(handle: &SessionHandle) -> SessionHandle {
    SessionHandle::new(handle.expose().into()).expect("live nonempty handle")
}

pub(crate) fn password() -> Result<Password, String> {
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
    // Protected setup keeps its custody in the system's protected store; a system without one
    // provisions nothing.
    if org_platform_host::credentials::protected_store().is_err() {
        return Err(
            "protected platform setup/custody is unavailable on this target; no HQ was provisioned"
                .into(),
        );
    }
    let account = crate::home::account_dir().ok_or("account home unavailable")?;
    // The instance's own reason: which folder, and the rule it broke (a data volume on a
    // memory filesystem, a folder of another user). Paths and fixed texts only.
    Paths::prepare(&account, user_temp(&account)?, name)
        .map_err(|why| format!("this account's instance folders cannot be used: {why}"))
}
fn user_temp(account: &std::path::Path) -> Result<PathBuf, String> {
    use org_platform::dirs::UserTempError;
    org_platform::dirs::user_temp_base(account).map_err(|error| match error {
        UserTempError::Unavailable => "OS private temporary directory unavailable".into(),
        UserTempError::Invalid(_) => unavailable(),
        UserTempError::Unsupported(_) => "protected platform setup is unavailable".into(),
    })
}
mod host;
pub(crate) use host::{host_is_named_by_user_alone, verify_host, verify_service_host};
mod recovery;
pub use recovery::recovery;
mod terminal;
