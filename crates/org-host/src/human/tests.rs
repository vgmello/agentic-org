use super::*;

/// Called only by the ignored real supervisor fixture. Credentials/source paths
/// are generated under that fixture's temporary root; no live roots or accounts.
pub(crate) fn adopt_and_assign_actual(
    root: &Path,
    socket: &Path,
    instance: &str,
    office: &org_host_control::provision::Provisioned,
    public: &Path,
) {
    adopt_and_assign_with_password(
        root,
        socket,
        instance,
        office,
        public,
        "synthetic test password",
    );
}

pub(crate) fn adopt_and_assign_with_password(
    root: &Path,
    socket: &Path,
    instance: &str,
    office: &org_host_control::provision::Provisioned,
    public: &Path,
    password: &str,
) -> (String, org_proto::gateway::OperationId) {
    use org_proto::selected_tasks::{
        self as selected, AssignmentCommand, AssignmentResult, NativeAssignment, NullableId,
        Revision,
    };
    let task = crate::ids::uuid_v7().unwrap();
    let sources = root.join("client-import");
    std::fs::create_dir(&sources).unwrap();
    let snapshot=format!("---\nschema_version: 1\nid: {task}\nnumber: 1\ntype: request\nteam_id: {}\nproject_id: null\ntitle: Native imported task\nstatus: todo\nrevision: 1\nassigned_team_id: null\nassignee_id: null\ncreated_at: null\ncreated_by_kind: unknown\ncreated_by_id: null\nupdated_at: null\nupdated_by_kind: unknown\nupdated_by_id: null\nextension: 1.234567890123456789E+012\n---\nOriginal café body.\n",office.home_team_id);
    let counter = format!(
        "scope_kind: team\nscope_id: {}\nentity_kind: task\nlast_number: 1\n",
        office.home_team_id
    );
    let allocation=format!("scope_kind: team\nscope_id: {}\nentity_kind: task\nnumber: 1\nentity_id: {task}\nallocated_at: 2026-01-01T00:00:00Z\n",office.home_team_id);
    let blob = |s: &str| staging::Blob {
        sha256: hash(s.as_bytes()),
        byte_length: s.len() as u64,
    };
    let manifest = adoption::Manifest {
        version: 1,
        resource_profile: "selected-task-resources/1".into(),
        task_id: task.clone(),
        canonical_path: format!("teams/{}/work/tasks/{task}.md", office.home_team_id),
        public_snapshot: blob(&snapshot),
        history_origin: adoption::HistoryOrigin::SnapshotOnly,
        events: vec![],
        public_blobs: vec![],
        number_counter: Some(blob(&counter)),
        number_allocation: Some(blob(&allocation)),
    };
    for source in [&snapshot, &counter, &allocation] {
        std::fs::write(sources.join(hash(source.as_bytes())), source).unwrap();
    }
    let manifest_path = root.join("import-manifest.json");
    std::fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    let op = crate::ids::generate().unwrap();
    let stage = crate::ids::generate().unwrap();
    let args = vec![
        "--host-socket".into(),
        socket.to_str().unwrap().into(),
        "--instance-id".into(),
        instance.into(),
        "--org-id".into(),
        office.organization_id.clone(),
        "--office-id".into(),
        office.office_id.clone(),
        "--login".into(),
        "Owner".into(),
        "--operation-id".into(),
        op.clone(),
        "--stage-id".into(),
        stage,
        "--directory-generation".into(),
        "1".into(),
        "--manifest".into(),
        manifest_path.to_str().unwrap().into(),
        "--sources".into(),
        sources.to_str().unwrap().into(),
    ];
    let credential = || Password::new(password.into()).map_err(str::to_owned);
    adopt_with_password(&args, credential).unwrap();
    let task_path = public.join(&manifest.canonical_path);
    assert_eq!(std::fs::read_to_string(&task_path).unwrap(), snapshot);
    adopt_with_password(&args, credential).unwrap();
    // Real native grant/route after adoption, with owner account independent of
    // any optional employee profile. Keep the host connection alive throughout.
    let mut host = Wire::new(UnixStream::connect(socket).unwrap()).unwrap();
    let hello = host
        .call::<Hello>(
            &HelloParams::offering(org_host_control::SUPPORTED_API, "test-native"),
            None,
        )
        .unwrap();
    assert_eq!(hello.instance_id.as_deref(), Some(instance));
    let account = host
        .call::<ops::AccountVerify>(
            &ops::AccountVerifyParams {
                login: "Owner".into(),
                password: credential().unwrap(),
            },
            None,
        )
        .unwrap();
    let opened = host
        .call::<ops::OfficeOpenChannel>(
            &ops::OfficeOpenChannelParams {
                session: session(&account.session),
                office_id: office.office_id.clone(),
            },
            None,
        )
        .unwrap();
    let (marker, mut fds) = host.byte(1).unwrap();
    assert_eq!(marker, ops::HOST_SOCK_MARKER);
    let mut worker =
        Wire::new(org_socket::rights::connected_unix_stream(fds.pop().unwrap()).unwrap()).unwrap();
    host.call::<ops::FdsReceived>(
        &ops::FdsReceivedParams {
            of: opened.channel_id.clone(),
        },
        None,
    )
    .unwrap();
    let hello = worker
        .call::<Hello>(
            &HelloParams::offering(org_proto::ApiRange { min: 2, max: 2 }, "test-native"),
            None,
        )
        .unwrap();
    assert!(hello.supports(selected::CAPABILITY));
    let principal = Principal {
        grant: host
            .call::<ops::SessionGrant>(
                &ops::SessionGrantParams {
                    session: session(&account.session),
                    office_id: office.office_id.clone(),
                    channel_id: opened.channel_id,
                },
                None,
            )
            .unwrap()
            .grant,
    };
    let change = NativeAssignment {
        change_id: org_proto::gateway::OperationId::parse(crate::ids::generate().unwrap()).unwrap(),
        task_id: task.clone(),
        command: AssignmentCommand {
            expected_revision: Revision::new(1).unwrap(),
            assigned_team_id: NullableId(Some(office.home_team_id.clone())),
            assignee_id: NullableId(None),
        },
    };
    let accepted = worker
        .call::<selected::SetAssignment>(&change, Some(&principal))
        .unwrap();
    assert!(matches!(
        accepted,
        AssignmentResult::Completed { revision: 2, .. }
    ));
    assert_eq!(
        worker
            .call::<selected::SetAssignment>(&change, Some(&principal))
            .unwrap(),
        accepted
    );
    let updated = std::fs::read_to_string(&task_path).unwrap();
    assert!(updated.contains("revision: 2"));
    assert!(updated.contains("extension: 1.234567890123456789E+012"));
    assert!(updated.ends_with("Original café body.\n"));
    assert!(public
        .join(format!("work-history/tasks/{task}/2.json"))
        .is_file());
    // A stale request has a durable receipt; it cannot be reinterpreted later.
    let receipt = change.change_id.clone();
    let mut stale = change;
    stale.change_id =
        org_proto::gateway::OperationId::parse(crate::ids::generate().unwrap()).unwrap();
    let rejected = worker
        .call::<selected::SetAssignment>(&stale, Some(&principal))
        .unwrap();
    assert!(
        matches!(&rejected,AssignmentResult::Rejected{code,current:Some(state)} if code=="conflict"&&state.revision==2)
    );
    assert_eq!(
        worker
            .call::<selected::SetAssignment>(&stale, Some(&principal))
            .unwrap(),
        rejected
    );
    // Exact adoption retry still returns its original revision after assignment.
    adopt_with_password(&args, credential).unwrap();
    assert_eq!(std::fs::read_to_string(&task_path).unwrap(), updated);
    (task, receipt)
}

#[test]
fn human_adoption_requires_explicit_scope_and_never_accepts_secret_arguments() {
    assert!(adopt(&["--password".into(), "not-used".into()]).is_err());
    assert!(adopt(&[]).is_err());
    assert!(adopt(&[
        "--instance-id".into(),
        "a".into(),
        "--instance-id".into(),
        "b".into()
    ])
    .is_err());
}
