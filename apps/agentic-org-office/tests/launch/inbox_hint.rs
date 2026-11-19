use super::*;

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
        org_office::Key::from_office_secrets_key(office_secrets_key()),
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
        // Hints travel only on `subscribe`, which this office does not serve yet (B6).
        assert!(!hello.supports("inbox.changed"), "{state}");
        drop(reader);
        assert!(run.host_goes_away().success());
    }
}
