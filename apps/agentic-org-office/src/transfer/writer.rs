//! The host's writer custody, as the office's store sees it (`org_store::WriterAuthority`),
//! over the private writer-control wire (version 2). A task never supplies this channel.
//!
//! The host sends its admission first: the writer identity it reserved for this office and
//! the organization's data roots. The store then asks for the confirmed head once at opening
//! and confirms its flushed head once per group flush. A denial, a broken frame or a closed
//! channel ends the channel for good: every later call fails closed.
use org_office::journal_office::{Custody, DataRoots};
use org_office_launch::writer::{self as wire, Admission, Head, HighWaters, Request, Response};
use org_store::{CustodyError, Id, JournalHead, OfficeIdentity, Sha256, WriterAuthority};
use std::{
    io::{self, BufReader},
    os::unix::net::UnixStream,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

/// How long one request may wait for the host's answer.
const ANSWER: Duration = Duration::from_secs(5);

/// The fixed text of every custody failure: never a path, an ID or the host's words.
fn refused() -> CustodyError {
    CustodyError("the host's writer custody refused or is unavailable".into())
}

/// What the host admitted this office as: its writer custody and its data roots.
pub(crate) struct Admitted {
    pub custody: Custody,
    pub roots: DataRoots,
    pub admission: Admission,
}

/// The office identity in the store's types: every ID a valid store ID, never reformatted.
fn identity(admission: &Admission) -> io::Result<OfficeIdentity> {
    let id = |text: &str| Id::parse(text).map_err(|_| io::Error::other("invalid writer identity"));
    let wire = &admission.identity;
    Ok(OfficeIdentity {
        instance_id: id(&wire.instance_id)?,
        organization_id: id(&wire.organization_id)?,
        office_id: id(&wire.office_id)?,
        journal_incarnation: id(&wire.journal_incarnation)?,
        writer_epoch: wire.writer_epoch,
        machine_id: id(&wire.machine_id)?,
    })
}

fn to_wire(head: &JournalHead) -> Head {
    Head {
        ordinal: head.ordinal,
        sha256: head.sha256.to_hex(),
        high_waters: HighWaters {
            organization: head.high_waters.organization,
            work: head.high_waters.work,
            message_content: head.high_waters.message_content,
            message_state: head.high_waters.message_state,
        },
    }
}

fn from_wire(head: Head) -> Result<JournalHead, CustodyError> {
    if !head.check() {
        return Err(refused());
    }
    Ok(JournalHead {
        ordinal: head.ordinal,
        sha256: Sha256::from_hex(&head.sha256).ok_or_else(refused)?,
        high_waters: org_store::HighWaters {
            organization: head.high_waters.organization,
            work: head.high_waters.work,
            message_content: head.high_waters.message_content,
            message_state: head.high_waters.message_state,
        },
    })
}

/// The host channel. One request at a time; the first failure closes it for good.
struct Authority {
    identity: OfficeIdentity,
    wire: wire::Identity,
    stream: Mutex<BufReader<UnixStream>>,
    closed: AtomicBool,
}

impl Authority {
    fn call(&self, identity: &OfficeIdentity, request: Request) -> Result<Response, CustodyError> {
        if identity != &self.identity || self.closed.load(Ordering::SeqCst) {
            return Err(refused());
        }
        let mut stream = self.stream.lock().map_err(|_| refused())?;
        let result = (|| {
            wire::write(stream.get_mut(), &request)?;
            wire::read(&mut *stream)?.ok_or_else(|| io::Error::other("writer control closed"))
        })();
        match result {
            Ok(response) if !matches!(response, Response::Denied) => Ok(response),
            _ => {
                self.close(&stream);
                Err(refused())
            }
        }
    }

    /// Fails closed: nothing on this channel is trusted again.
    fn close(&self, stream: &BufReader<UnixStream>) {
        self.closed.store(true, Ordering::SeqCst);
        let _ = stream.get_ref().shutdown(std::net::Shutdown::Both);
    }

    fn unexpected(&self) -> CustodyError {
        if let Ok(stream) = self.stream.lock() {
            self.close(&stream);
        }
        refused()
    }
}

impl WriterAuthority for Authority {
    fn checkpoint(&self, identity: &OfficeIdentity) -> Result<Option<JournalHead>, CustodyError> {
        let request = Request::Checkpoint {
            identity: self.wire.clone(),
        };
        match self.call(identity, request)? {
            Response::Checkpoint { head } => head
                .map(from_wire)
                .transpose()
                .map_err(|_| self.unexpected()),
            _ => Err(self.unexpected()),
        }
    }

    fn confirm(
        &self,
        identity: &OfficeIdentity,
        before: Option<&JournalHead>,
        after: &JournalHead,
    ) -> Result<(), CustodyError> {
        let request = Request::Confirm {
            identity: self.wire.clone(),
            before: before.map(to_wire),
            after: to_wire(after),
        };
        match self.call(identity, request)? {
            Response::Confirmed => Ok(()),
            _ => Err(self.unexpected()),
        }
    }
}

/// Receives the host's admission on `stream` and checks it: its own rules
/// ([`Admission::check`], the owner account's form included), this instance and this
/// office. Returns the custody the store writes under and the roots it writes to. The
/// office checks the roots and the organization against its creation intent.
pub(crate) fn install(stream: UnixStream, instance: &str, office_id: &str) -> io::Result<Admitted> {
    stream.set_read_timeout(Some(ANSWER))?;
    stream.set_write_timeout(Some(ANSWER))?;
    let mut reader = BufReader::new(stream);
    let admission: Admission =
        wire::read(&mut reader)?.ok_or_else(|| io::Error::other("missing writer admission"))?;
    if !admission.check()
        || admission.identity.instance_id != instance
        || admission.identity.office_id != office_id
    {
        return Err(io::Error::other("writer scope mismatch"));
    }
    let identity = identity(&admission)?;
    let authority = Arc::new(Authority {
        identity: identity.clone(),
        wire: admission.identity.clone(),
        stream: Mutex::new(reader),
        closed: AtomicBool::new(false),
    });
    Ok(Admitted {
        custody: Custody {
            identity,
            authority,
        },
        roots: DataRoots {
            durable: PathBuf::from(&admission.durable_root),
            runtime: PathBuf::from(&admission.runtime_root),
        },
        admission,
    })
}
