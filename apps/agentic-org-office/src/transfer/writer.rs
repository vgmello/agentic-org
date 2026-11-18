//! Authenticated host writer custody adapter. A task never supplies this channel.
use org_office::{
    AcceptanceConfig, AcceptanceIdentity, JournalPosition, Office, OfficeError, OwnerAccount,
    OwnerAuthority, WriterAuthority,
};
use org_office_launch::writer::{self as wire, Admission, Request, Response};
use std::{
    io::{self, BufReader},
    os::unix::net::UnixStream,
    sync::{Arc, Mutex},
    time::Duration,
};

struct Authority {
    admission: Admission,
    stream: Mutex<BufReader<UnixStream>>,
}
fn invalid() -> OfficeError {
    OfficeError::Invalid("protected writer admission unavailable")
}
impl Authority {
    fn call(
        &self,
        identity: &AcceptanceIdentity,
        request: Request,
    ) -> Result<Response, OfficeError> {
        if identity != &self.admission.identity {
            return Err(invalid());
        }
        let mut stream = self.stream.lock().map_err(|_| invalid())?;
        let result = (|| {
            wire::write(stream.get_mut(), &request)?;
            wire::read(&mut *stream)?.ok_or_else(|| io::Error::other("writer control closed"))
        })();
        match result {
            Ok(response) if !matches!(response, Response::Denied) => Ok(response),
            _ => {
                let _ = stream.get_ref().shutdown(std::net::Shutdown::Both);
                Err(invalid())
            }
        }
    }
}
impl WriterAuthority for Authority {
    fn checkpoint(
        &self,
        identity: &AcceptanceIdentity,
    ) -> Result<Option<JournalPosition>, OfficeError> {
        match self.call(
            identity,
            Request::Checkpoint {
                identity: identity.clone(),
            },
        )? {
            Response::Checkpoint { position } => Ok(position),
            _ => Err(invalid()),
        }
    }
    fn confirm(
        &self,
        identity: &AcceptanceIdentity,
        before: Option<&JournalPosition>,
        after: &JournalPosition,
    ) -> Result<(), OfficeError> {
        match self.call(
            identity,
            Request::Confirm {
                identity: identity.clone(),
                before: before.cloned(),
                after: after.clone(),
            },
        )? {
            Response::Confirmed => Ok(()),
            _ => Err(invalid()),
        }
    }
}
impl OwnerAuthority for Authority {
    fn verify_live(&self, identity: &AcceptanceIdentity) -> Result<OwnerAccount, OfficeError> {
        match self.call(
            identity,
            Request::Owner {
                identity: identity.clone(),
            },
        )? {
            Response::Owner { account, member }
                if account == self.admission.owner_account
                    && member == self.admission.owner_member =>
            {
                Ok(OwnerAccount {
                    account_id: account,
                    member_id: member,
                })
            }
            _ => Err(invalid()),
        }
    }
}
pub(super) fn install(
    stream: UnixStream,
    office: &Office,
    instance: &str,
    office_id: &str,
) -> io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    let mut reader = BufReader::new(stream);
    let admission: Admission =
        wire::read(&mut reader)?.ok_or_else(|| io::Error::other("missing writer admission"))?;
    if admission.version != wire::VERSION
        || admission.identity.instance_id != instance
        || admission.identity.office_id != office_id
    {
        return Err(io::Error::other("writer scope mismatch"));
    }
    let authority = Arc::new(Authority {
        admission: admission.clone(),
        stream: Mutex::new(reader),
    });
    let config = AcceptanceConfig {
        identity: admission.identity.clone(),
        protected_root: admission.protected_root.clone().into(),
        public_root: admission.public_root.clone().into(),
        authority: Some(authority.clone()),
    };
    office
        .validate_hq_creation_admission(&config, &admission.owner_member)
        .and_then(|()| office.install_selected_acceptance(config))
        .and_then(|()| office.bind_selected_owner(authority.clone()))
        .and_then(|()| office.finish_selected_bootstrap(authority))
        // Keep the office error (its text has no path, SQL or secret) so the caller can log why
        // admission failed; the host still sees only the fixed failure code.
        .map_err(io::Error::other)
}
