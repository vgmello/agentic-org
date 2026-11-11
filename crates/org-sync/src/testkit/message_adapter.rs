use super::*;
use crate::messages::{
    Message, MessageAttempt, MessageAuthority, MessageOutput, MessageStore, MessageTxn, Queue,
    Receipt,
};

impl MessageStore for MemoryOffice {
    fn transact(&self, attempt: Arc<MessageAttempt>) -> Result<MessageOutput, SyncError> {
        self.atomic(|tx| attempt(tx))
    }
}
fn authorized_route(peer: &PeerIdentity, endpoint: &OfficeId, hq: &OfficeId) -> bool {
    let is_hq = &peer.office_id == hq;
    let role = if is_hq {
        PeerRole::Hq
    } else {
        PeerRole::Satellite
    };
    peer.role == role && (&peer.office_id == endpoint || is_hq)
}

impl Tx<'_> {
    fn message_io(&mut self, name: &str) -> Result<(), StoreError> {
        if let Some((target, remaining)) = self.message_failure.as_mut() {
            if *target == name {
                if *remaining == 0 {
                    self.message_failure = None;
                    return Err(StoreError::unavailable(
                        "injected private message store cause",
                    ));
                }
                *remaining -= 1;
            }
        }
        Ok(())
    }
}

impl MessageTxn for Tx<'_> {
    fn restore_pending(&mut self) -> Result<bool, StoreError> {
        self.message_io("restore_pending")?;
        Ok(self.data.checkpoint.restore_pending)
    }
    fn authority(&mut self) -> Result<MessageAuthority, StoreError> {
        self.message_io("authority")?;
        let c = &self.data.checkpoint;
        Ok(MessageAuthority {
            org_id: c.org_id.clone(),
            office_id: c.local_office.clone(),
            epoch: c.highest_epoch,
        })
    }
    fn supports_current_protocol(&mut self) -> Result<bool, StoreError> {
        self.message_io("supports_current_protocol")?;
        Ok(
            crate::versions::protocol_profile(self.data.checkpoint.protocol_level).is_some()
                && self.data.checkpoint.protocol_level <= self.data.supported_max,
        )
    }
    fn queue(&mut self) -> Result<Queue, StoreError> {
        self.message_io("queue")?;
        Ok(self.data.message_queue.clone())
    }
    fn outgoing_receipt(&mut self, id: &str) -> Result<Option<Receipt>, StoreError> {
        self.message_io("outgoing_receipt")?;
        Ok(self.data.outgoing_messages.get(id).cloned())
    }
    fn received(&mut self, origin: &OfficeId, id: &str) -> Result<Option<Receipt>, StoreError> {
        self.message_io("received")?;
        Ok(self
            .data
            .received_messages
            .get(&(origin.clone(), id.to_owned()))
            .cloned())
    }
    fn authorize_send(&mut self, m: &Message) -> Result<(), SyncError> {
        self.message_io("authorize_send")?;
        if m.fingerprint != fingerprint(m) {
            return Err(Refusal::InvalidInput.into());
        }
        Ok(())
    }
    fn authorize_receive(&mut self, peer: &PeerIdentity, m: &Message) -> Result<(), SyncError> {
        self.message_io("authorize_receive")?;
        self.authorize_send(m)?;
        if !authorized_route(peer, &m.origin, &self.data.checkpoint.hq_office) {
            return Err(Refusal::NotAuthorized.into());
        }
        Ok(())
    }
    fn authorize_receipt(&mut self, peer: &PeerIdentity, r: &Receipt) -> Result<(), SyncError> {
        self.message_io("authorize_receipt")?;
        if !authorized_route(
            peer,
            &r.message.destination,
            &self.data.checkpoint.hq_office,
        ) {
            return Err(Refusal::NotAuthorized.into());
        }
        Ok(())
    }
    fn apply_message(&mut self, message: &Message) -> Result<(), SyncError> {
        self.message_io("apply_message")?;
        self.data.delivered_messages.push(message.payload.clone());
        if self.domain_refusal {
            return Err(Refusal::InvalidInput.into());
        }
        Ok(())
    }
    fn record_received(&mut self, receipt: &Receipt) -> Result<(), StoreError> {
        self.message_io("record_received")?;
        self.data.received_messages.insert(
            (receipt.message.origin.clone(), receipt.message.id.clone()),
            receipt.clone(),
        );
        Ok(())
    }
    fn save_queue(&mut self, queue: &Queue, receipt: Option<&Receipt>) -> Result<(), StoreError> {
        self.message_io("save_queue")?;
        self.data.message_queue = queue.clone();
        if let Some(r) = receipt {
            self.data
                .outgoing_messages
                .insert(r.message.id.clone(), r.clone());
        }
        Ok(())
    }
}
pub(super) fn fingerprint(m: &Message) -> Digest {
    let fields = format!("{}:{}:{}:{}:", m.origin, m.destination, m.id, m.deadline_ms);
    let mut bytes = fields.into_bytes();
    bytes.extend_from_slice(&m.payload);
    org_sync_proto::event_digest_v1(
        &m.org_id,
        Position {
            epoch: Epoch::new(1).unwrap(),
            seq: 1,
        },
        Digest::ZERO,
        &Codec::new("message.fixture", 1).unwrap(),
        &bytes,
    )
    .unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messages;
    fn offices() -> (MemoryOffice, MemoryOffice, MemoryPeer, MemoryPeer, Message) {
        let org = OrgId::new("ORG1").unwrap();
        let hq = OfficeId::new("HQ1").unwrap();
        let sat = OfficeId::new("SAT1").unwrap();
        let a = MemoryOffice::new(
            org.clone(),
            hq.clone(),
            hq.clone(),
            Digest::ZERO,
            Epoch::new(1).unwrap(),
        );
        let b = MemoryOffice::new(
            org.clone(),
            sat.clone(),
            hq.clone(),
            Digest::ZERO,
            Epoch::new(1).unwrap(),
        );
        let pa = MemoryPeer(PeerIdentity {
            authority: Epoch::new(1).unwrap(),
            org_id: org.clone(),
            office_id: hq.clone(),
            role: PeerRole::Hq,
        });
        let pb = MemoryPeer(PeerIdentity {
            authority: Epoch::new(1).unwrap(),
            org_id: org.clone(),
            office_id: sat.clone(),
            role: PeerRole::Satellite,
        });
        let mut message = Message {
            org_id: org,
            origin: hq,
            destination: sat,
            id: "MSG1".into(),
            fingerprint: Digest::ZERO,
            payload: Arc::from(&b"hello"[..]),
            deadline_ms: 1000,
        };
        message.fingerprint = fingerprint(&message);
        (a, b, pa, pb, message)
    }
    #[test]
    fn oversize_message_is_refused_before_enqueue() {
        let (a, _, _, _, mut message) = offices();
        message.payload = Arc::from(vec![0; org_sync_proto::MAX_ENCODED_FRAME_BYTES / 2 + 1]);
        message.fingerprint = fingerprint(&message);
        let before = a.data.borrow().message_queue.clone();
        assert!(matches!(
            messages::enqueue(&a, message, 10),
            Err(SyncError::Refused(Refusal::InvalidInput))
        ));
        assert_eq!(a.data.borrow().message_queue, before);
        assert!(a.data.borrow().outgoing_messages.is_empty());
    }

    #[test]
    fn delivery_and_ack_commit_atomically_and_lost_ack_survives_restart() {
        let (a, b, pa, pb, m) = offices();
        a.inject(Fault::CommitUnavailable);
        assert!(messages::enqueue(&a, m.clone(), 10).is_err());
        assert!(a.data.borrow().message_queue.pending.is_empty());
        messages::enqueue(&a, m.clone(), 10).unwrap();
        b.inject(Fault::CommitUnavailable);
        assert!(messages::receive(&b, &pa, m.clone(), 10).is_err());
        assert!(b.data.borrow().delivered_messages.is_empty());
        assert!(b.data.borrow().received_messages.is_empty());
        let MessageOutput::Receipt(receipt) = messages::receive(&b, &pa, m.clone(), 10).unwrap()
        else {
            panic!("receipt")
        };
        let b = b.restart();
        assert_eq!(
            messages::receive(&b, &pa, m.clone(), 2000).unwrap(),
            MessageOutput::Receipt(receipt.clone())
        );
        assert_eq!(b.data.borrow().delivered_messages.len(), 1);
        a.inject(Fault::CommitUnavailable);
        assert!(messages::acknowledge(&a, &pb, receipt.clone()).is_err());
        assert_eq!(a.data.borrow().message_queue.pending.len(), 1);
        messages::acknowledge(&a, &pb, receipt.clone()).unwrap();
        assert_eq!(
            messages::enqueue(&a.restart(), m, 10).unwrap(),
            MessageOutput::Receipt(receipt)
        );
        assert!(a.data.borrow().message_queue.pending.is_empty());
    }
    #[test]
    fn foreign_peer_cannot_observe_the_restore_fence_via_message_receive_or_ack() {
        let (a, b, pa, pb, m) = offices();
        messages::enqueue(&a, m.clone(), 10).unwrap();
        let MessageOutput::Receipt(receipt) = messages::receive(&b, &pa, m.clone(), 10).unwrap()
        else {
            panic!("receipt")
        };
        let foreign = |peer: &MemoryPeer| {
            MemoryPeer(PeerIdentity {
                org_id: OrgId::new("OTHER").unwrap(),
                ..peer.identity()
            })
        };
        for pending in [false, true] {
            for office in [&a, &b] {
                office.data.borrow_mut().checkpoint.restore_pending = pending;
            }
            for result in [
                messages::receive(&b, &foreign(&pa), m.clone(), 10),
                messages::acknowledge(&a, &foreign(&pb), receipt.clone()),
            ] {
                assert!(matches!(
                    result,
                    Err(SyncError::Refused(Refusal::NotAuthorized))
                ));
            }
        }
        assert_eq!(a.data.borrow().message_queue.pending.len(), 1);
        assert_eq!(b.data.borrow().delivered_messages.len(), 1);
    }
    #[test]
    fn wrong_message_route_role_or_local_endpoint_cannot_observe_restore_fence() {
        let (a, b, pa, pb, m) = offices();
        messages::enqueue(&a, m.clone(), 10).unwrap();
        let MessageOutput::Receipt(receipt) = messages::receive(&b, &pa, m.clone(), 10).unwrap()
        else {
            panic!("receipt")
        };
        let wrong_role = MemoryPeer(PeerIdentity {
            role: PeerRole::Satellite,
            ..pa.identity()
        });
        let wrong_route = MemoryPeer(PeerIdentity {
            office_id: OfficeId::new("OTHER").unwrap(),
            ..pb.identity()
        });
        let mut wrong_endpoint = m.clone();
        wrong_endpoint.destination = OfficeId::new("OTHER").unwrap();
        wrong_endpoint.fingerprint = fingerprint(&wrong_endpoint);
        for pending in [false, true] {
            for office in [&a, &b] {
                office.data.borrow_mut().checkpoint.restore_pending = pending;
            }
            for result in [
                messages::receive(&b, &wrong_role, m.clone(), 10),
                messages::receive(&b, &pa, wrong_endpoint.clone(), 10),
                messages::acknowledge(&a, &wrong_route, receipt.clone()),
            ] {
                assert!(matches!(
                    result,
                    Err(SyncError::Refused(Refusal::NotAuthorized))
                ));
            }
        }
        assert_eq!(a.data.borrow().message_queue.pending.len(), 1);
        assert_eq!(b.data.borrow().delivered_messages.len(), 1);
    }
    #[test]
    fn direct_message_delivery_and_ack_require_the_pinned_peer_role() {
        let (a, b, pa, pb, m) = offices();
        messages::enqueue(&a, m.clone(), 10).unwrap();
        let wrong_sender = MemoryPeer(PeerIdentity {
            role: PeerRole::Satellite,
            ..pa.identity()
        });
        let bad_delivery = messages::receive(&b, &wrong_sender, m.clone(), 10);
        let MessageOutput::Receipt(receipt) = messages::receive(&b, &pa, m, 10).unwrap() else {
            panic!("valid receipt")
        };
        let wrong_recipient = MemoryPeer(PeerIdentity {
            role: PeerRole::Hq,
            ..pb.identity()
        });
        let bad_ack = messages::acknowledge(&a, &wrong_recipient, receipt.clone());
        for result in [bad_delivery, bad_ack] {
            assert!(matches!(
                result,
                Err(SyncError::Refused(Refusal::NotAuthorized))
            ));
        }
        assert_eq!(b.data.borrow().delivered_messages.len(), 1);
        assert_eq!(a.data.borrow().message_queue.pending.len(), 1);
        messages::acknowledge(&a, &pb, receipt).unwrap();
    }
    #[test]
    fn message_receive_and_ack_require_current_verified_authority() {
        struct Signature;
        impl AuthorityVerifier for Signature {
            fn verify(&self, _: &AuthorityRecord) -> Result<(), Refusal> {
                Ok(())
            }
        }
        let (a, b, pa, pb, m) = offices();
        messages::enqueue(&a, m.clone(), 10).unwrap();
        let epoch = Epoch::new(2).unwrap();
        for office in [&a, &b] {
            let meta = office.checkpoint();
            accept_authority(
                office,
                AuthorityRecord {
                    org_id: meta.org_id,
                    hq_office: meta.hq_office,
                    ca: meta.ca,
                    epoch,
                    restored_position: meta.position,
                    restored_digest: meta.digest,
                    signature: Arc::from([]),
                },
                Arc::new(Signature),
            )
            .unwrap();
        }
        assert!(matches!(
            messages::receive(&b, &pa, m.clone(), 10),
            Err(SyncError::Refused(Refusal::NotAuthorized))
        ));
        let pa = MemoryPeer::at_epoch(&pa, epoch);
        let MessageOutput::Receipt(receipt) = messages::receive(&b, &pa, m, 10).unwrap() else {
            panic!("current authority receipt")
        };
        assert!(matches!(
            messages::acknowledge(&a, &pb, receipt.clone()),
            Err(SyncError::Refused(Refusal::NotAuthorized))
        ));
        assert_eq!(a.data.borrow().message_queue.pending.len(), 1);
        messages::acknowledge(&a, &MemoryPeer::at_epoch(&pb, epoch), receipt).unwrap();
        assert!(a.data.borrow().message_queue.pending.is_empty());
    }
    #[test]
    fn conflicting_message_identity_and_wrong_ack_route_refuse() {
        let (a, b, pa, pb, m) = offices();
        messages::enqueue(&a, m.clone(), 100).unwrap();
        assert_eq!(
            messages::next(&a, 100).unwrap(),
            MessageOutput::Deliver(m.clone())
        );
        let MessageOutput::Receipt(receipt) = messages::receive(&b, &pa, m.clone(), 100).unwrap()
        else {
            panic!("receipt")
        };
        assert!(matches!(
            messages::acknowledge(
                &a,
                &MemoryPeer(PeerIdentity {
                    authority: Epoch::new(1).unwrap(),
                    org_id: m.org_id.clone(),
                    office_id: OfficeId::new("OTHER").unwrap(),
                    role: PeerRole::Satellite
                }),
                receipt.clone()
            ),
            Err(SyncError::Refused(Refusal::NotAuthorized))
        ));
        messages::acknowledge(&a, &pb, receipt).unwrap();
        let mut changed = m;
        changed.payload = Arc::from(&b"different"[..]);
        changed.fingerprint = fingerprint(&changed);
        assert!(matches!(
            messages::receive(&b, &pa, changed.clone(), 100),
            Err(SyncError::Refused(Refusal::Divergence))
        ));
        assert!(matches!(
            messages::enqueue(&a, changed, 100),
            Err(SyncError::Refused(Refusal::OperationReused))
        ));
        assert_eq!(b.data.borrow().delivered_messages.len(), 1);
    }
    #[test]
    fn expiry_retains_identity_and_late_proven_delivery_can_correct_an_expiry_notice() {
        let (a, b, pa, pb, m) = offices();
        messages::enqueue(&a, m.clone(), 100).unwrap();
        let MessageOutput::Receipt(delivered) = messages::receive(&b, &pa, m.clone(), 100).unwrap()
        else {
            panic!("receipt")
        };
        let MessageOutput::Receipt(expired) = messages::next(&a, 1000).unwrap() else {
            panic!("expired")
        };
        assert!(!expired.delivered);
        assert_eq!(
            messages::enqueue(&a, m.clone(), 10).unwrap(),
            MessageOutput::Receipt(expired)
        );
        assert_eq!(
            messages::acknowledge(&a, &pb, delivered.clone()).unwrap(),
            MessageOutput::Receipt(delivered)
        );
        let mut new = m;
        new.id = "MSG2".into();
        new.fingerprint = fingerprint(&new);
        assert_eq!(
            messages::enqueue(&a, new, 10).unwrap(),
            MessageOutput::Refused(Refusal::Expired)
        );
    }
    #[derive(Clone, Debug, PartialEq, Eq)]
    struct Saved {
        checkpoint: OrgCheckpoint,
        initial_snapshot_required: bool,
        queue: Queue,
        outgoing: BTreeMap<String, Receipt>,
        received: BTreeMap<(OfficeId, String), Receipt>,
        delivered: Vec<Arc<[u8]>>,
    }
    fn state(office: &MemoryOffice) -> Saved {
        let d = office.data.borrow();
        Saved {
            checkpoint: d.checkpoint.clone(),
            initial_snapshot_required: d.initial_snapshot_required,
            queue: d.message_queue.clone(),
            outgoing: d.outgoing_messages.clone(),
            received: d.received_messages.clone(),
            delivered: d.delivered_messages.clone(),
        }
    }
    fn refused(result: Result<MessageOutput, SyncError>, reason: Refusal) {
        assert!(
            matches!(&result, Err(SyncError::Refused(actual)) if *actual == reason),
            "expected {reason:?}, got {result:?}"
        );
    }

    #[test]
    fn invalid_message_shapes_refuse_before_every_write_without_changing_state() {
        for case in [
            "empty_id",
            "long_id",
            "unicode",
            "slash",
            "empty_body",
            "large_body",
            "zero_deadline",
        ] {
            for action in ["enqueue", "receive", "ack"] {
                let (a, b, pa, pb, original) = offices();
                messages::enqueue(&a, original.clone(), 10).unwrap();
                let mut m = original.clone();
                match case {
                    "empty_id" => m.id.clear(),
                    "long_id" => m.id = "M".repeat(129),
                    "unicode" => m.id = "é".into(),
                    "slash" => m.id = "a/b".into(),
                    "empty_body" => m.payload = Arc::from([]),
                    "large_body" => m.payload = Arc::from(vec![0; messages::MAX_PAYLOAD_BYTES + 1]),
                    _ => m.deadline_ms = 0,
                }
                m.fingerprint = fingerprint(&m);
                let before_a = state(&a);
                let before_b = state(&b);
                let result = match action {
                    "enqueue" => messages::enqueue(&a, m, 10),
                    "receive" => messages::receive(&b, &pa, m, 10),
                    _ => messages::acknowledge(
                        &a,
                        &pb,
                        Receipt {
                            message: m,
                            delivered: true,
                        },
                    ),
                };
                refused(result, Refusal::InvalidInput);
                assert_eq!(state(&a), before_a, "{case}/{action}");
                assert_eq!(state(&b), before_b, "{case}/{action}");
            }
        }
    }

    #[test]
    fn cap_boundary_and_allowed_native_id_characters_round_trip_after_reopen() {
        let (a, _, _, _, mut m) = offices();
        assert_eq!(
            messages::MAX_PAYLOAD_BYTES,
            org_sync_proto::MAX_ENCODED_FRAME_BYTES / 2
        );
        m.id = format!("_-{}", "M".repeat(126));
        m.payload = Arc::from(vec![1; messages::MAX_PAYLOAD_BYTES]);
        m.fingerprint = fingerprint(&m);
        assert_eq!(
            messages::enqueue(&a, m.clone(), 10).unwrap(),
            MessageOutput::Queued { id: m.id.clone() }
        );
        let a = a.restart();
        assert_eq!(messages::next(&a, 10).unwrap(), MessageOutput::Deliver(m));
    }

    fn broken_queue(a: &MemoryOffice, m: &Message, case: &str) -> Queue {
        let mut queue = a.data.borrow().message_queue.clone();
        let mut row = m.clone();
        row.id = "OLD".into();
        match case {
            "binding_org" => queue.org_id = OrgId::new("OTHER").unwrap(),
            "binding_office" => queue.office_id = OfficeId::new("OTHER").unwrap(),
            "count" => {
                for i in 0..=messages::MAX_PENDING {
                    let mut value = row.clone();
                    value.id = format!("OLD{i}");
                    value.deadline_ms = 1;
                    queue.pending.push_back(value);
                }
            }
            "bytes" => {
                row.payload = Arc::from(vec![0; messages::MAX_PAYLOAD_BYTES]);
                for i in 0..=messages::MAX_PENDING_BYTES / messages::MAX_PAYLOAD_BYTES {
                    let mut value = row.clone();
                    value.id = format!("OLD{i}");
                    queue.pending.push_back(value);
                }
            }
            "duplicate" => queue.pending.extend([row.clone(), row]),
            _ => {
                match case {
                    "org" => row.org_id = OrgId::new("OTHER").unwrap(),
                    "origin" => row.origin = OfficeId::new("OTHER").unwrap(),
                    "id" => row.id.clear(),
                    "body" => row.payload = Arc::from([]),
                    _ => row.deadline_ms = 0,
                }
                queue.pending.push_back(row);
            }
        }
        queue
    }

    #[test]
    fn the_same_shape_and_identity_rules_guard_loads_and_saves() {
        for case in [
            "binding_org",
            "binding_office",
            "count",
            "bytes",
            "duplicate",
            "org",
            "origin",
            "id",
            "body",
            "deadline",
        ] {
            for action in ["enqueue", "next", "receive", "ack", "persist"] {
                let (a, _, _, pb, m) = offices();
                let receipt = Receipt {
                    message: m.clone(),
                    delivered: true,
                };
                let mut incoming = m.clone();
                std::mem::swap(&mut incoming.origin, &mut incoming.destination);
                incoming.fingerprint = fingerprint(&incoming);
                {
                    let mut d = a.data.borrow_mut();
                    d.outgoing_messages.insert(m.id.clone(), receipt.clone());
                    d.received_messages.insert(
                        (incoming.origin.clone(), incoming.id.clone()),
                        Receipt {
                            message: incoming.clone(),
                            delivered: true,
                        },
                    );
                }
                let broken = broken_queue(&a, &m, case);
                a.data.borrow_mut().message_queue = broken.clone();
                let before = state(&a);
                let result = match action {
                    "enqueue" => messages::enqueue(&a, m, 10),
                    "next" => messages::next(&a, 10),
                    "receive" => messages::receive(&a, &pb, incoming, 10),
                    "ack" => messages::acknowledge(&a, &pb, receipt),
                    _ => MessageStore::transact(
                        &a,
                        Arc::new(move |tx| {
                            messages::persist(tx, &broken, None)?;
                            Ok(MessageOutput::Idle)
                        }),
                    ),
                };
                let error = result.expect_err("corrupt queue must abort, including proof replay");
                assert!(
                    matches!(error, SyncError::Refused(Refusal::InvariantFailure)),
                    "{case}/{action}: {error:?}"
                );
                assert!(error.aborts_batch());
                assert_eq!(state(&a), before, "{case}/{action}");
            }
        }
    }

    #[test]
    fn a_storage_failure_at_each_boundary_never_exposes_a_partial_result() {
        let cases: &[(&str, &[(&str, usize)])] = &[
            (
                "enqueue",
                &[
                    ("queue", 0),
                    ("authority", 0),
                    ("restore_pending", 0),
                    ("supports_current_protocol", 0),
                    ("authorize_send", 0),
                    ("outgoing_receipt", 0),
                    ("authority", 1),
                    ("save_queue", 0),
                ],
            ),
            (
                "next",
                &[
                    ("queue", 0),
                    ("authority", 0),
                    ("restore_pending", 0),
                    ("supports_current_protocol", 0),
                    ("authority", 1),
                    ("save_queue", 0),
                ],
            ),
            (
                "receive",
                &[
                    ("authority", 0),
                    ("authorize_receive", 0),
                    ("authorize_send", 0),
                    ("restore_pending", 0),
                    ("supports_current_protocol", 0),
                    ("queue", 0),
                    ("authority", 1),
                    ("received", 0),
                    ("apply_message", 0),
                    ("record_received", 0),
                    ("authority", 2),
                    ("save_queue", 0),
                ],
            ),
            (
                "ack",
                &[
                    ("authority", 0),
                    ("authorize_receipt", 0),
                    ("restore_pending", 0),
                    ("supports_current_protocol", 0),
                    ("queue", 0),
                    ("authority", 1),
                    ("outgoing_receipt", 0),
                    ("authority", 2),
                    ("save_queue", 0),
                ],
            ),
        ];
        for (action, failures) in cases {
            for (port, skip) in *failures {
                let (a, b, pa, pb, m) = offices();
                messages::enqueue(&a, m.clone(), 10).unwrap();
                let office = if *action == "receive" { &b } else { &a };
                let mut new = m.clone();
                new.id = "MSG2".into();
                new.fingerprint = fingerprint(&new);
                let before = state(office);
                office.inject(Fault::MessagePort(port, *skip));
                let result = match *action {
                    "enqueue" => messages::enqueue(&a, new, 20),
                    "next" => messages::next(&a, 20),
                    "receive" => messages::receive(&b, &pa, m.clone(), 20),
                    _ => messages::acknowledge(
                        &a,
                        &pb,
                        Receipt {
                            message: m.clone(),
                            delivered: true,
                        },
                    ),
                };
                assert!(
                    matches!(result, Err(SyncError::Storage(_))),
                    "{action}/{port}/{skip}: {result:?}"
                );
                assert_eq!(state(office), before, "{action}/{port}/{skip}");
            }
        }
    }

    #[test]
    fn debug_reports_lengths_without_payload_bytes_at_any_nesting() {
        let (a, _, _, _, m) = offices();
        let bytes = format!("{:?}", m.payload);
        let receipt = Receipt {
            message: m.clone(),
            delivered: true,
        };
        messages::enqueue(&a, m.clone(), 10).unwrap();
        for text in [
            format!("{m:?}"),
            format!("{receipt:?}"),
            format!("{:?}", a.data.borrow().message_queue),
            format!("{:?}", MessageOutput::Deliver(m)),
        ] {
            assert!(text.contains("payload_bytes"));
            assert!(!text.contains(&bytes), "{text}");
        }
    }
    #[test]
    fn restore_and_protocol_fences_refuse_every_operation_without_writes() {
        for fence in ["restore", "protocol"] {
            for action in ["enqueue", "next", "receive", "ack"] {
                let (a, b, pa, pb, m) = offices();
                messages::enqueue(&a, m.clone(), 10).unwrap();
                let office = if action == "receive" { &b } else { &a };
                {
                    let mut d = office.data.borrow_mut();
                    if fence == "restore" {
                        d.checkpoint.restore_pending = true;
                    } else {
                        d.checkpoint.protocol_level = 2;
                    }
                }
                let before = state(office);
                let result = match action {
                    "enqueue" => messages::enqueue(&a, m.clone(), 20),
                    "next" => messages::next(&a, 20),
                    "receive" => messages::receive(&b, &pa, m.clone(), 20),
                    _ => messages::acknowledge(
                        &a,
                        &pb,
                        Receipt {
                            message: m.clone(),
                            delivered: true,
                        },
                    ),
                };
                refused(
                    result,
                    if fence == "restore" {
                        Refusal::RestorePending
                    } else {
                        Refusal::UnsupportedVersion
                    },
                );
                assert_eq!(state(office), before, "{fence}/{action}");
            }
        }
    }

    #[test]
    fn invalid_remote_identity_refuses_before_restore_state_or_retained_proofs() {
        for action in ["receive", "ack"] {
            for wrong in [
                "peer_org",
                "epoch",
                "office",
                "role",
                "message_org",
                "endpoint",
            ] {
                let (a, b, pa, pb, mut m) = offices();
                let (office, mut identity) = if action == "receive" {
                    (&b, pa.identity())
                } else {
                    (&a, pb.identity())
                };
                office.data.borrow_mut().checkpoint.restore_pending = true;
                match wrong {
                    "peer_org" => identity.org_id = OrgId::new("OTHER").unwrap(),
                    "epoch" => identity.authority = Epoch::new(2).unwrap(),
                    "office" => identity.office_id = OfficeId::new("OTHER").unwrap(),
                    "role" => {
                        identity.role = if action == "receive" {
                            PeerRole::Satellite
                        } else {
                            PeerRole::Hq
                        }
                    }
                    "message_org" => m.org_id = OrgId::new("OTHER").unwrap(),
                    _ if action == "receive" => m.destination = OfficeId::new("OTHER").unwrap(),
                    _ => m.origin = OfficeId::new("OTHER").unwrap(),
                }
                m.fingerprint = fingerprint(&m);
                let before = state(office);
                let peer = MemoryPeer(identity);
                let result = if action == "receive" {
                    messages::receive(office, &peer, m, 10)
                } else {
                    messages::acknowledge(
                        office,
                        &peer,
                        Receipt {
                            message: m,
                            delivered: true,
                        },
                    )
                };
                refused(result, Refusal::NotAuthorized);
                assert_eq!(state(office), before, "{action}/{wrong}");
            }
        }
    }

    #[test]
    fn lost_post_commit_ack_replays_the_original_receipt_without_enqueue_or_reapply() {
        let (_, b, pa, _, m) = offices();
        let MessageOutput::Receipt(original) = messages::receive(&b, &pa, m.clone(), 10).unwrap()
        else {
            panic!("committed receipt");
        };
        let b = b.restart();
        let before = state(&b);
        let MessageOutput::Receipt(replay) = messages::receive(&b, &pa, m, 2000).unwrap() else {
            panic!("replayed receipt");
        };
        assert_eq!(replay, original);
        assert_eq!(
            replay.message.payload.as_ref(),
            original.message.payload.as_ref()
        );
        assert_eq!(state(&b), before);
        assert!(b.data.borrow().message_queue.pending.is_empty());
        assert_eq!(b.data.borrow().delivered_messages.len(), 1);
    }

    #[test]
    fn pending_replay_conflicts_and_ack_only_remove_the_matching_identity() {
        let (a, b, pa, pb, first) = offices();
        let mut second = first.clone();
        second.id = "MSG2".into();
        second.fingerprint = fingerprint(&second);
        for m in [&first, &second] {
            assert_eq!(
                messages::enqueue(&a, m.clone(), 10).unwrap(),
                MessageOutput::Queued { id: m.id.clone() }
            );
        }
        let a = a.restart();
        let before = state(&a);
        assert_eq!(
            messages::enqueue(&a, first.clone(), 10).unwrap(),
            MessageOutput::Queued {
                id: first.id.clone()
            }
        );
        assert_eq!(state(&a), before);
        let mut changed = first.clone();
        changed.payload = Arc::from(&b"changed"[..]);
        changed.fingerprint = fingerprint(&changed);
        refused(
            messages::enqueue(&a, changed.clone(), 10),
            Refusal::OperationReused,
        );
        refused(
            messages::acknowledge(
                &a,
                &pb,
                Receipt {
                    message: changed,
                    delivered: true,
                },
            ),
            Refusal::Divergence,
        );
        assert_eq!(state(&a), before);
        let MessageOutput::Receipt(receipt) = messages::receive(&b, &pa, first, 10).unwrap() else {
            panic!("receipt");
        };
        messages::acknowledge(&a, &pb, receipt.clone()).unwrap();
        assert_eq!(
            a.data.borrow().message_queue.pending,
            VecDeque::from([second])
        );
        let before = state(&a);
        assert_eq!(
            messages::acknowledge(&a, &pb, receipt.clone()).unwrap(),
            MessageOutput::Receipt(receipt.clone())
        );
        assert_eq!(state(&a), before);
        let mut conflict = receipt;
        conflict.message.deadline_ms += 1;
        conflict.message.fingerprint = fingerprint(&conflict.message);
        refused(
            messages::acknowledge(&a, &pb, conflict),
            Refusal::Divergence,
        );
        assert_eq!(state(&a), before);
    }

    #[test]
    fn false_or_unknown_ack_and_native_fingerprint_refuse_without_writes() {
        let (a, b, pa, pb, m) = offices();
        messages::enqueue(&a, m.clone(), 10).unwrap();
        let before = state(&a);
        refused(
            messages::acknowledge(
                &a,
                &pb,
                Receipt {
                    message: m.clone(),
                    delivered: false,
                },
            ),
            Refusal::NotAuthorized,
        );
        let mut unknown = m.clone();
        unknown.id = "UNKNOWN".into();
        unknown.fingerprint = fingerprint(&unknown);
        refused(
            messages::acknowledge(
                &a,
                &pb,
                Receipt {
                    message: unknown,
                    delivered: true,
                },
            ),
            Refusal::InvalidInput,
        );
        let mut bad = m;
        bad.fingerprint = Digest::ZERO;
        refused(
            messages::enqueue(&a, bad.clone(), 10),
            Refusal::InvalidInput,
        );
        refused(messages::receive(&b, &pa, bad, 10), Refusal::InvalidInput);
        assert_eq!(state(&a), before);
        assert!(b.data.borrow().delivered_messages.is_empty());
        assert!(b.data.borrow().received_messages.is_empty());
    }

    #[test]
    fn quota_edges_refuse_before_write_and_the_exact_byte_budget_reopens() {
        for case in ["count", "bytes", "exact_bytes"] {
            let (a, _, _, _, mut m) = offices();
            let payload_size = if case == "count" {
                1
            } else {
                messages::MAX_PAYLOAD_BYTES
            };
            let count = if case == "count" {
                messages::MAX_PENDING
            } else {
                messages::MAX_PENDING_BYTES / payload_size - usize::from(case == "exact_bytes")
            };
            let payload: Arc<[u8]> = Arc::from(vec![1; payload_size]);
            {
                let mut data = a.data.borrow_mut();
                for i in 0..count {
                    let mut row = m.clone();
                    row.id = format!("P{i}");
                    row.payload = payload.clone();
                    row.fingerprint = fingerprint(&row);
                    data.message_queue.pending.push_back(row);
                }
            }
            m.payload = if case == "exact_bytes" {
                payload
            } else {
                Arc::from(&b"x"[..])
            };
            m.fingerprint = fingerprint(&m);
            let before = state(&a);
            let result = messages::enqueue(&a, m.clone(), 10);
            if case == "exact_bytes" {
                assert_eq!(result.unwrap(), MessageOutput::Queued { id: m.id });
                let reopened = a.restart();
                assert_eq!(
                    reopened.data.borrow().message_queue.pending.len(),
                    count + 1
                );
                assert!(matches!(
                    messages::next(&reopened, 10).unwrap(),
                    MessageOutput::Deliver(_)
                ));
            } else {
                refused(result, Refusal::QuotaExceeded);
                assert_eq!(state(&a), before, "{case}");
            }
        }
    }

    #[test]
    fn idle_and_expiry_use_the_durable_clock_and_only_remove_the_head() {
        let (a, _, _, _, m) = offices();
        a.inject(Fault::MessagePort("save_queue", 0));
        let before = state(&a);
        assert!(matches!(
            messages::next(&a, 2000),
            Err(SyncError::Storage(_))
        ));
        assert_eq!(state(&a), before);
        assert_eq!(messages::next(&a, 20).unwrap(), MessageOutput::Idle);
        assert_eq!(a.restart().data.borrow().message_queue.clock_high_ms, 20);
        messages::enqueue(&a, m.clone(), 20).unwrap();
        let mut second = m.clone();
        second.id = "MSG2".into();
        second.deadline_ms = 2000;
        second.fingerprint = fingerprint(&second);
        messages::enqueue(&a, second.clone(), 20).unwrap();
        a.data.borrow_mut().message_queue.clock_high_ms = m.deadline_ms;
        let MessageOutput::Receipt(expired) = messages::next(&a, 1).unwrap() else {
            panic!("expiry follows durable high water");
        };
        assert!(!expired.delivered);
        assert_eq!(expired.message, m);
        assert_eq!(
            a.data.borrow().message_queue.pending,
            VecDeque::from([second.clone()])
        );
        assert_eq!(
            messages::next(&a.restart(), 1).unwrap(),
            MessageOutput::Deliver(second)
        );
    }

    #[test]
    fn receive_expiry_is_fenced_by_persisted_time_without_inventing_delivery() {
        let (_, b, pa, _, m) = offices();
        b.data.borrow_mut().message_queue.clock_high_ms = m.deadline_ms;
        assert_eq!(
            messages::receive(&b, &pa, m.clone(), 1).unwrap(),
            MessageOutput::Refused(Refusal::Expired)
        );
        assert!(b.data.borrow().delivered_messages.is_empty());
        assert!(b.data.borrow().received_messages.is_empty());
        assert_eq!(
            b.restart().data.borrow().message_queue.clock_high_ms,
            m.deadline_ms
        );
    }

    #[test]
    fn a_new_signed_authority_refuses_stale_sessions_before_its_restore_fence() {
        struct Signature;
        impl AuthorityVerifier for Signature {
            fn verify(&self, _: &AuthorityRecord) -> Result<(), Refusal> {
                Ok(())
            }
        }
        let (a, b, pa, pb, m) = offices();
        let event = a.publish(b"ahead of restored HQ").unwrap();
        apply(&b, &pa, event).unwrap();
        messages::enqueue(&a, m.clone(), 10).unwrap();
        let epoch = Epoch::new(2).unwrap();
        for office in [&a, &b] {
            let c = office.checkpoint();
            accept_authority(
                office,
                AuthorityRecord {
                    org_id: c.org_id,
                    hq_office: c.hq_office,
                    ca: c.ca,
                    epoch,
                    restored_position: Position {
                        epoch: Epoch::new(1).unwrap(),
                        seq: 0,
                    },
                    restored_digest: Digest::ZERO,
                    signature: Arc::from([]),
                },
                Arc::new(Signature),
            )
            .unwrap();
        }
        let before_a = state(&a);
        let before_b = state(&b);
        refused(
            messages::receive(&b, &pa, m.clone(), 10),
            Refusal::NotAuthorized,
        );
        refused(
            messages::receive(&b, &MemoryPeer::at_epoch(&pa, epoch), m.clone(), 10),
            Refusal::RestorePending,
        );
        let receipt = Receipt {
            message: m,
            delivered: true,
        };
        refused(
            messages::acknowledge(&a, &pb, receipt.clone()),
            Refusal::NotAuthorized,
        );
        refused(
            messages::acknowledge(&a, &MemoryPeer::at_epoch(&pb, epoch), receipt),
            Refusal::RestorePending,
        );
        assert_eq!(state(&a), before_a);
        assert_eq!(state(&b), before_b);
    }

    #[test]
    fn native_apply_refusal_and_organization_snapshot_preserve_message_state() {
        let (a, b, pa, _, m) = offices();
        messages::enqueue(&a, m.clone(), 10).unwrap();
        let before_b = state(&b);
        b.inject(Fault::DomainRefusal);
        refused(messages::receive(&b, &pa, m, 10), Refusal::InvalidInput);
        assert_eq!(state(&b), before_b);
        let before_a = state(&a);
        let c = a.checkpoint();
        a.stage("ORGONLY", vec![Arc::from(&b"organization projection"[..])]);
        install_snapshot(
            &a,
            ValidatedSnapshot {
                org_id: c.org_id,
                authority: c.highest_epoch,
                position: c.position,
                digest: c.digest,
                codec: Codec::new("conformance", 1).unwrap(),
                staging_id: "ORGONLY".into(),
                protocol_level: 1,
            },
        )
        .unwrap();
        assert_eq!(state(&a), before_a);
        assert_eq!(state(&a.restart()), before_a);
    }
    #[test]
    fn local_message_scope_mismatch_is_refused_before_queue_writes() {
        for field in ["org", "origin"] {
            let (a, _, _, _, mut m) = offices();
            if field == "org" {
                m.org_id = OrgId::new("OTHER").unwrap();
            } else {
                m.origin = OfficeId::new("OTHER").unwrap();
            }
            m.fingerprint = fingerprint(&m);
            let before = state(&a);
            refused(messages::enqueue(&a, m, 10), Refusal::NotAuthorized);
            assert_eq!(state(&a), before);
        }
        assert_eq!(Refusal::OperationReused.code(), "operation_reused");
        assert_eq!(Refusal::Expired.code(), "expired");
    }

    #[test]
    fn expiry_write_errors_never_return_a_terminal_notice_or_corrected_receipt() {
        for action in ["enqueue", "receive", "next", "late_ack"] {
            let (a, b, pa, pb, m) = offices();
            let receipt = Receipt {
                message: m.clone(),
                delivered: true,
            };
            if action == "next" || action == "late_ack" {
                messages::enqueue(&a, m.clone(), 10).unwrap();
            }
            if action == "late_ack" {
                messages::receive(&b, &pa, m.clone(), 10).unwrap();
                messages::next(&a, m.deadline_ms).unwrap();
            }
            let office = if action == "receive" { &b } else { &a };
            let before = state(office);
            office.inject(Fault::MessagePort("save_queue", 0));
            let response = match action {
                "enqueue" => messages::enqueue(&a, m.clone(), m.deadline_ms),
                "receive" => messages::receive(&b, &pa, m.clone(), m.deadline_ms),
                "next" => messages::next(&a, m.deadline_ms),
                _ => messages::acknowledge(&a, &pb, receipt.clone()),
            };
            assert!(
                matches!(response, Err(SyncError::Storage(_))),
                "{action}: {response:?}"
            );
            assert_eq!(state(office), before, "{action}");
            match action {
                "enqueue" => assert_eq!(
                    messages::enqueue(&a, m.clone(), m.deadline_ms).unwrap(),
                    MessageOutput::Refused(Refusal::Expired)
                ),
                "receive" => assert_eq!(
                    messages::receive(&b, &pa, m.clone(), m.deadline_ms).unwrap(),
                    MessageOutput::Refused(Refusal::Expired)
                ),
                "next" => assert!(matches!(
                    messages::next(&a, m.deadline_ms).unwrap(),
                    MessageOutput::Receipt(_)
                )),
                _ => assert_eq!(
                    messages::acknowledge(&a, &pb, receipt.clone()).unwrap(),
                    MessageOutput::Receipt(receipt)
                ),
            }
            assert_eq!(
                office.restart().data.borrow().message_queue.clock_high_ms,
                m.deadline_ms
            );
        }
    }
    #[test]
    fn an_undelivered_received_proof_aborts_instead_of_becoming_a_replay_ack() {
        let (_, b, pa, _, m) = offices();
        b.data.borrow_mut().received_messages.insert(
            (m.origin.clone(), m.id.clone()),
            Receipt {
                message: m.clone(),
                delivered: false,
            },
        );
        let before = state(&b);
        let error = messages::receive(&b, &pa, m, 10)
            .expect_err("retained delivery proof must mean committed delivery");
        assert!(matches!(
            error,
            SyncError::Refused(Refusal::InvariantFailure)
        ));
        assert!(error.aborts_batch());
        assert_eq!(state(&b), before);
    }
}
