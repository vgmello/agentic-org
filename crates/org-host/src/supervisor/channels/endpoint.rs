//! One office's channel endpoint: opening channels to its worker with bounded pending
//! receipts, and closing them when the office stops.

use std::{
    io::{self, BufReader},
    os::{
        fd::{AsFd, OwnedFd},
        unix::net::UnixStream,
    },
    sync::{atomic::Ordering, mpsc, Arc},
    time::{Duration, Instant},
};

use crate::supervisor::signal::Signal;
use crate::{cloexec::SPAWN_LOCK, sync::lock};

#[cfg(test)]
use org_proto::GrantKey;
#[cfg(test)]
use std::{
    os::unix::net::UnixDatagram,
    sync::{atomic::AtomicBool, Mutex},
};

use super::registry::ChannelSlot;
use super::{invalid, launch, Endpoint, Pending, Receipt, DEADLINE, MAX_PENDING};

impl Endpoint {
    /// A separate one-shot channel to the already authenticated live worker.
    /// It shares descriptor/channel budgets, never the writer-control stream.
    pub(crate) fn recovery_public(
        self: &Arc<Self>,
        request: &org_office_launch::recovery::Request,
    ) -> io::Result<org_office_launch::recovery::PublicRecord> {
        use org_office_launch::recovery as wire;
        use sha2::{Digest, Sha256};
        request.check()?;
        if request.office_id != self.office {
            return Err(invalid());
        }
        let (_, mut client, _slot) =
            self.ordinary_channel(org_office_launch::ChannelKind::Local, |id| {
                org_office_launch::TransferRecord::RecoveryPublicRead {
                    version: 1,
                    office_id: self.office.clone(),
                    channel_id: id.into(),
                }
            })?;
        client.set_nonblocking(false)?;
        client.set_read_timeout(Some(DEADLINE))?;
        client.set_write_timeout(Some(DEADLINE))?;
        wire::write_request(&mut client, request)?;
        client.shutdown(std::net::Shutdown::Write)?;
        let record = wire::read_record(&mut BufReader::new(client))?;
        let bytes = record.package()?;
        if !self.is_live()
            || record.binding() != *request
            || record.package_sha256 != wire::hex(&Sha256::digest(bytes))
        {
            return Err(invalid());
        }
        Ok(record)
    }
    #[cfg(test)]
    pub(crate) fn for_grant_test() -> (Arc<Self>, ChannelSlot, UnixDatagram, UnixStream) {
        let (transfer, worker) = UnixDatagram::pair().unwrap();
        let (close, peer) = UnixStream::pair().unwrap();
        let (signals, _events) = mpsc::channel();
        let endpoint = Arc::new(Self {
            office: "office".into(),
            key: GrantKey::from_bytes([71; 32]),
            transfer,
            close,
            live: Arc::new(AtomicBool::new(true)),
            pending: Mutex::default(),
            channels: Mutex::default(),
            signals,
            generation: 1,
        });
        let slot = {
            let mut channels = lock(&endpoint.channels);
            let slot = channels
                .reserve("channel", org_office_launch::ChannelKind::Local)
                .unwrap();
            assert!(channels.acknowledge("channel"));
            slot
        };
        (endpoint, slot, worker, peer)
    }
    pub(super) fn writer_control(&self, worker: UnixStream) -> io::Result<()> {
        let id = crate::ids::generate()?;
        let header = org_office_launch::encode_transfer_record(
            &org_office_launch::TransferRecord::WriterControl {
                office_id: self.office.clone(),
                channel_id: id.clone(),
            },
        )
        .map_err(launch)?;
        self.transfer_descriptor(&id, worker, &header, false)
    }
    fn transfer_descriptor(
        &self,
        id: &str,
        worker: UnixStream,
        header: &[u8],
        ordinary: bool,
    ) -> io::Result<()> {
        let descriptor = Arc::new(OwnedFd::from(worker));
        let (sent, receipt) = mpsc::channel();
        {
            let mut pending = lock(&self.pending);
            if !self.is_live() || pending.len() >= MAX_PENDING || pending.contains_key(id) {
                return Err(invalid());
            }
            pending.insert(
                id.into(),
                Pending {
                    descriptor: Arc::clone(&descriptor),
                    receipt: sent,
                    ordinary,
                },
            );
        }
        // Arc retains this single descriptor during send without holding either registry lock.
        let send = org_socket::rights::send(
            self.transfer.as_fd(),
            header,
            &[descriptor.as_fd()],
            Instant::now() + DEADLINE,
        );
        drop(descriptor); // Only the pending ownership record now retains the sender copy.
        if send.is_err() {
            self.fail();
            return Err(invalid());
        }
        match receipt.recv_timeout(DEADLINE) {
            Ok(Receipt::Accepted) => Ok(()),
            Ok(Receipt::Capacity) => Err(invalid()),
            Err(_) => {
                self.fail();
                Err(invalid())
            }
        }
    }
    pub(super) fn receipt(&self, event: org_office_launch::RendezvousEvent) -> io::Result<()> {
        let (id, refused) = match event {
            org_office_launch::RendezvousEvent::ChannelClosed { of } => {
                return lock(&self.channels)
                    .remove(&of, true)
                    .then_some(())
                    .ok_or_else(invalid);
            }
            org_office_launch::RendezvousEvent::FdsReceived { of } => (of, false),
            org_office_launch::RendezvousEvent::FdsRefused {
                of,
                reason: org_office_launch::RefusalReason::Capacity,
            } => (of, true),
            _ => return Err(invalid()),
        };
        let mut pending = lock(&self.pending);
        let record = pending.get(&id).ok_or_else(invalid)?;
        if refused && !record.ordinary {
            return Err(invalid());
        }
        if record.ordinary {
            let mut channels = lock(&self.channels);
            let valid = if refused {
                channels.remove(&id, false)
            } else {
                channels.acknowledge(&id)
            };
            if !valid {
                return Err(invalid());
            }
        }
        let Pending {
            descriptor,
            receipt,
            ..
        } = pending.remove(&id).ok_or_else(invalid)?;
        drop(descriptor);
        drop(pending);
        let _ = receipt.send(if refused {
            Receipt::Capacity
        } else {
            Receipt::Accepted
        });
        Ok(())
    }
    pub fn is_live(&self) -> bool {
        self.live.load(Ordering::SeqCst)
    }
    pub fn fail(&self) {
        let first = self.live.swap(false, Ordering::SeqCst);
        lock(&self.channels).fail();
        lock(&self.pending).clear();
        if first {
            let _ = self.close.shutdown(std::net::Shutdown::Both);
            let _ = self.signals.send(Signal::Closed {
                generation: self.generation,
            });
        }
    }
    pub fn open(
        self: &Arc<Self>,
        kind: org_office_launch::ChannelKind,
    ) -> io::Result<(String, UnixStream, ChannelSlot)> {
        self.ordinary_channel(kind, |id| org_office_launch::TransferRecord::Channel {
            kind,
            office_id: self.office.clone(),
            channel_id: id.into(),
        })
    }
    fn ordinary_channel(
        &self,
        kind: org_office_launch::ChannelKind,
        record: impl FnOnce(&str) -> org_office_launch::TransferRecord,
    ) -> io::Result<(String, UnixStream, ChannelSlot)> {
        let id = crate::ids::generate()?;
        let slot = lock(&self.channels).reserve(&id, kind)?;
        let result = (|| {
            let (client, worker) = {
                let _fence = lock(&SPAWN_LOCK);
                UnixStream::pair()?
            };
            let header = org_office_launch::encode_transfer_record(&record(&id)).map_err(launch)?;
            self.transfer_descriptor(&id, worker, &header, true)?;
            Ok((id.clone(), client, slot))
        })();
        if result.is_err() {
            // Only a not-yet-acknowledged reservation may be rolled back here.
            lock(&self.channels).remove(&id, false);
        }
        result
    }
    pub fn revoke(&self, session: &str) -> io::Result<()> {
        let header = org_office_launch::encode_transfer_record(
            &org_office_launch::TransferRecord::Revoked {
                session_ids: vec![session.into()],
            },
        )
        .map_err(launch)?;
        if !self.live.load(Ordering::SeqCst)
            || org_socket::rights::send(
                self.transfer.as_fd(),
                &header,
                &[],
                Instant::now() + Duration::from_millis(100),
            )
            .is_err()
        {
            self.fail();
            return Err(invalid());
        }
        Ok(())
    }
}

impl Drop for Endpoint {
    fn drop(&mut self) {
        self.fail();
    }
}
