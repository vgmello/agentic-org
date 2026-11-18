use super::*;

#[test]
#[ignore = "rewrite on A3's grant-bound host-channel subscribe (/events): no office-socket subscriber exists"]
/// Must be rewritten on A3's grant-bound host-channel subscribe (/events): since A1.5 nobody
/// binds or subscribes as the owner on the office socket. Scenario to keep: with the owner and a
/// non-owner subscribed, a `msg.send` to the owner delivers one `inbox.changed` to the owner
/// within 1 s (counts as `inbox.list` had them, as_of seq + 1, not degraded) and none to the
/// non-owner; replaying the same send commits nothing and emits no second hint.
fn inbox_hint_real_mailbox_commit_advances_once_and_filters_the_same_hint_per_delivery() {
    panic!("rewrite on A3's grant-bound host-channel subscribe; see the doc comment");
}

#[test]
#[ignore = "rewrite on A3's grant-bound host-channel subscribe (/events): no office-socket subscriber exists"]
/// Must be rewritten on A3's grant-bound host-channel subscribe (/events): since A1.5 nobody
/// binds or subscribes as the owner on the office socket. Scenario to keep: an office seeded with
/// an agent placed in no office shows one problem in `inbox.list`; a second owner session's
/// `member.offboard` of it delivers `inbox.changed` within 1 s with zero counts, as_of seq + 1,
/// not degraded, and `inbox.list` agrees with no items.
fn inbox_hint_real_second_owner_offboard_clears_the_seeded_problem_within_one_second() {
    panic!("rewrite on A3's grant-bound host-channel subscribe; see the doc comment");
}

fn local_office(world: &World, id: &str) -> org_office::Office {
    org_office::Office::create(
        world.folder(id),
        &org_office::NewOffice {
            office_id: id.into(),
            office_name: id.into(),
            role: org_office::OfficeRole::Hq,
            org_id: ORG.into(),
            org_name: "Acme".into(),
        },
        org_office::Key::from_db_key(db_key()),
    )
    .unwrap()
}

#[test]
fn inbox_hint_registration_uses_real_unbound_bound_restore_and_replica_authority() {
    use org_sync::organization::{OrgOutput, OrgStore};
    let _processes = process_isolation();
    for state in ["unbound", "hq", "restore", "satellite"] {
        let world = World::new();
        let office = local_office(&world, OFFICE);
        if state == "satellite" {
            let source = local_office(&world, "SOURCE");
            source
                .bind_replication_hq(Default::default(), 1024 * 1024)
                .unwrap();
            let pin = source.replication_checkpoint().unwrap();
            office
                .bind_replication_satellite(
                    pin.hq_office,
                    pin.ca,
                    pin.highest_epoch,
                    Default::default(),
                )
                .unwrap();
        } else if state != "unbound" {
            office
                .bind_replication_hq(Default::default(), 1024 * 1024)
                .unwrap();
            if state == "restore" {
                // Use the existing trusted store contract to seed the local journal fence.
                office
                    .transact(std::sync::Arc::new(|tx| {
                        let mut checkpoint = tx.checkpoint()?;
                        checkpoint.restore_pending = true;
                        tx.set_authority(&checkpoint)?;
                        Ok(OrgOutput::AuthorityAccepted {
                            snapshot_required: false,
                        })
                    }))
                    .unwrap();
            }
        }
        drop(office);
        let mut run = started(&world);
        // A raw synchronous hello owns no detached client helpers across the next spawn.
        let mut reader = BufReader::new(UnixStream::connect(socket(&world)).unwrap());
        frame::write_message(
            reader.get_mut(),
            &serde_json::json!({"id":1,"method":"hello","params":{}}),
        )
        .unwrap();
        let reply: org_proto::Response = frame::read_message(&mut reader).unwrap().unwrap();
        let hello = reply.into_result::<org_proto::Hello>().unwrap();
        assert_eq!(
            hello.supports("inbox.list"),
            state != "satellite",
            "{state}"
        );
        assert_eq!(
            hello.supports("inbox.changed"),
            state != "satellite",
            "{state}"
        );
        drop(reader);
        assert!(run.host_goes_away().success());
    }
}
