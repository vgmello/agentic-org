use org_store::StoreError;

use super::*;

fn id(n: u64) -> Id {
    Id::parse(&format!("01900000-0000-7000-8000-{n:012x}")).unwrap()
}

fn meta() -> Meta {
    Meta::created(&By::Employee(id(9)), Timestamp(1_700_000_000_000))
}

fn team() -> Team {
    Team {
        id: id(4),
        name: "Core".into(),
        number: 1,
        parent_team_id: None,
        lead_employee_id: Some(id(9)),
        status: TeamStatus::Active,
        max_members: 6,
        meta: meta(),
    }
}

fn employee() -> Employee {
    Employee {
        id: id(9),
        handle: "ana".into(),
        kind: EmployeeKind::Person,
        name: "Ana".into(),
        home_team_id: id(4),
        role_id: None,
        reports_to_employee_id: None,
        status: EmployeeStatus::Active,
        contact: None,
        title: Some("Founder".into()),
        focus: None,
        role_text: None,
        title_in_team: None,
        offboarded_at: None,
        may_hire: true,
        builtin_key: None,
        meta: meta(),
        body: "About Ana.\n".into(),
    }
}

/// `bytes` with the line of `key` replaced by `line` (removed when empty).
fn with_line(bytes: &[u8], key: &str, line: &str) -> Vec<u8> {
    let text = std::str::from_utf8(bytes).unwrap();
    let start = format!("{key}: ");
    assert!(text.lines().any(|l| l.starts_with(&start)), "no {key}");
    let lines = text.split_inclusive('\n').map(|l| match l {
        l if l.starts_with(&start) => line.to_owned(),
        l => l.to_owned(),
    });
    lines.collect::<String>().into_bytes()
}

#[test]
fn a_team_file_has_every_field_once_in_byte_order_and_reads_back() {
    let bytes = encode_team(&team());
    assert_eq!(
        String::from_utf8(bytes.clone()).unwrap(),
        "created_at: \"2023-11-14T22:13:20.000Z\"\n\
         created_by_id: \"01900000-0000-7000-8000-000000000009\"\n\
         created_by_kind: \"employee\"\n\
         id: \"01900000-0000-7000-8000-000000000004\"\n\
         lead_employee_id: \"01900000-0000-7000-8000-000000000009\"\n\
         max_members: 6\n\
         name: \"Core\"\n\
         number: 1\n\
         parent_team_id: null\n\
         revision: 1\n\
         schema_version: 1\n\
         status: \"active\"\n\
         updated_at: \"2023-11-14T22:13:20.000Z\"\n\
         updated_by_id: \"01900000-0000-7000-8000-000000000009\"\n\
         updated_by_kind: \"employee\"\n"
    );
    assert_eq!(decode_team(&bytes), Ok(team()));
}

#[test]
fn every_record_reads_back_as_it_was_written() {
    let organization = Organization {
        id: id(1),
        slug: "example-org".into(),
        name: "Example \"Org\": Zoë".into(),
        organization_repository_id: id(3),
        state_branch: "org/state".into(),
        meta: meta(),
    };
    let bytes = encode_organization(&organization);
    assert_eq!(decode_organization(&bytes), Ok(organization));

    let system = Meta::created(&By::System, Timestamp(1));
    let repositories = vec![
        Repository {
            id: id(3),
            key: Some("organization".into()),
            kind: RepositoryKind::Remote,
            remote_url: Some("https://example.test/org.git".into()),
            meta: meta(),
        },
        Repository {
            id: id(5),
            key: None,
            kind: RepositoryKind::Local,
            remote_url: None,
            meta: system.clone(),
        },
    ];
    let bytes = encode_repositories(&repositories);
    assert_eq!(decode_repositories(&bytes), Ok(repositories));
    assert_eq!(decode_repositories(&encode_repositories(&[])), Ok(vec![]));

    let office = Office {
        id: id(2),
        slug: "hq".into(),
        name: "London HQ".into(),
        role: OfficeRole::Hq,
        meta: Meta {
            created_at: None,
            created_by: By::Unknown,
            ..system
        },
    };
    assert_eq!(decode_office(&encode_office(&office)), Ok(office));

    let codes = vec![
        OfficeCode {
            code: "A".into(),
            office_id: id(2),
            issued_at: "2026-11-14T10:00:00Z".into(),
            retired_at: Some("2026-11-15T10:00:00Z".into()),
        },
        OfficeCode {
            code: "ZZ".into(),
            office_id: id(2),
            issued_at: "2026-11-15T10:00:00Z".into(),
            retired_at: None,
        },
    ];
    let bytes = encode_office_codes(&codes);
    assert_eq!(decode_office_codes(&bytes), Ok(codes));

    let bytes = encode_employee(&employee());
    assert!(bytes.starts_with(b"---\nbuiltin_key: null\ncontact: null\n"));
    assert!(bytes.ends_with(b"updated_by_kind: \"employee\"\n---\nAbout Ana.\n"));
    assert_eq!(decode_employee(&bytes), Ok(employee()));

    let counter = TeamCounter {
        organization_id: id(1),
        last_number: 7,
    };
    let bytes = encode_team_counter(&counter);
    assert_eq!(decode_team_counter(&bytes), Ok(counter.clone()));
    assert_eq!(counter.advance().unwrap().last_number, 8);
    let full = TeamCounter {
        last_number: i64::MAX as u64,
        ..counter
    };
    assert_eq!(full.advance(), Err(StoreError::Exhausted("team numbers")));

    let allocation = TeamAllocation {
        organization_id: id(1),
        number: 7,
        team_id: id(4),
        allocated_at: "2026-11-14T10:00:00Z".into(),
    };
    let bytes = encode_team_allocation(&allocation);
    assert_eq!(decode_team_allocation(&bytes), Ok(allocation));
}

#[test]
fn a_file_with_a_missing_an_unknown_or_a_wrong_field_is_refused_by_field() {
    let good = encode_team(&team());
    let refused = |key: &str, line: &str| {
        let error = decode_team(&with_line(&good, key, line)).unwrap_err();
        (error.field, error.reason)
    };
    let is = |field: &str, reason: &str| (field.to_owned(), reason.to_owned());
    assert_eq!(refused("name", ""), is("name", "missing"));
    assert_eq!(
        refused("name", "name: \"Core\"\nnickname: \"C\"\n"),
        is("nickname", "unknown field")
    );
    assert_eq!(refused("name", "name: 7\n"), is("name", "not a string"));
    assert_eq!(
        refused("name", "name: \" Core\"\n"),
        is("name", "not one line of text")
    );
    assert_eq!(refused("number", "number: 0\n"), is("number", "below 1"));
    assert_eq!(
        refused("id", "id: \"01900000-0000-7000-8000-00000000000A\"\n"),
        is("id", "not a UUIDv7 in lowercase text")
    );
    assert_eq!(
        refused("id", "id: \"01900000-0000-4000-8000-000000000004\"\n"),
        is("id", "not a UUIDv7 in lowercase text")
    );
    assert_eq!(
        refused(
            "parent_team_id",
            "parent_team_id: \"01900000-0000-7000-8000-000000000004\"\n"
        ),
        is("parent_team_id", "a team is not its own parent")
    );
    assert_eq!(
        refused("status", "status: \"closed\"\n"),
        is("status", "not one of: active archived ")
    );
    assert_eq!(
        refused("schema_version", "schema_version: 2\n"),
        is("schema_version", "not 1")
    );
    assert_eq!(
        refused("revision", "revision: 0\n"),
        is("revision", "below 1")
    );
    assert_eq!(
        refused("created_at", "created_at: \"yesterday\"\n"),
        is("created_at", "not an RFC 3339 time")
    );
    assert_eq!(
        refused("created_by_id", "created_by_id: null\n"),
        is("created_by_id", "an employee has an ID")
    );
    assert_eq!(
        refused("updated_by_kind", "updated_by_kind: \"system\"\n"),
        is("updated_by_id", "only an employee has an ID")
    );
    assert_eq!(
        refused("updated_by_kind", "updated_by_kind: \"owner\"\n"),
        is("updated_by_kind", "not one of: employee system unknown")
    );
}

#[test]
fn a_role_reads_back_and_its_body_is_the_markdown() {
    let role = Role {
        id: id(5),
        key: Some("engineer".into()),
        name: "Engineer".into(),
        meta: meta(),
        body: "Builds things.\n".into(),
    };
    let bytes = encode_role(&role);
    assert!(bytes.ends_with(b"---\nBuilds things.\n"));
    assert_eq!(decode_role(&bytes), Ok(role.clone()));
    let unkeyed = Role { key: None, ..role };
    assert_eq!(decode_role(&encode_role(&unkeyed)), Ok(unkeyed));
}

#[test]
fn a_team_takes_at_least_one_member_and_builtin_key_is_the_hr_agents_alone() {
    let team = encode_team(&team());
    let error = decode_team(&with_line(&team, "max_members", "max_members: 0\n")).unwrap_err();
    assert_eq!(
        (error.field.as_str(), error.reason.as_str()),
        ("max_members", "below 1")
    );
    assert!(decode_team(&with_line(&team, "max_members", "")).is_err());

    let hr = Employee {
        kind: EmployeeKind::Agent,
        may_hire: false,
        builtin_key: Some(BuiltinKey::Hr),
        ..employee()
    };
    let bytes = encode_employee(&hr);
    assert_eq!(decode_employee(&bytes), Ok(hr.clone()));
    let refused = |key: &str, line: &str| {
        let error = decode_employee(&with_line(&bytes, key, line)).unwrap_err();
        (error.field, error.reason)
    };
    let only = (
        "builtin_key".to_owned(),
        "only an agent, and the HR lead without may_hire".to_owned(),
    );
    assert_eq!(refused("kind", "kind: \"person\"\n"), only);
    assert_eq!(refused("may_hire", "may_hire: true\n"), only);
    let other = refused("builtin_key", "builtin_key: \"ceo\"\n");
    assert_eq!(other.0, "builtin_key");

    // The Chief of Staff is an agent that may hold may_hire, on or off; never a person.
    for may_hire in [true, false] {
        let tess = Employee {
            kind: EmployeeKind::Agent,
            may_hire,
            builtin_key: Some(BuiltinKey::ChiefOfStaff),
            ..employee()
        };
        let bytes = encode_employee(&tess);
        assert!(String::from_utf8_lossy(&bytes).contains("builtin_key: \"chief_of_staff\""));
        assert_eq!(decode_employee(&bytes), Ok(tess));
        let error = decode_employee(&with_line(&bytes, "kind", "kind: \"person\"\n")).unwrap_err();
        assert_eq!((error.field, error.reason), only.clone());
    }
    for text in ["Chief_Of_Staff", "chief-of-staff", "chief_of_staff ", ""] {
        assert_eq!(BuiltinKey::parse(text), None, "{text:?}");
    }
    for key in BuiltinKey::ALL {
        assert_eq!(BuiltinKey::parse(key.as_str()), Some(key));
    }
}

#[test]
fn rules_that_span_fields_of_one_file_are_refused() {
    let good = encode_employee(&employee());
    let refused = |key: &str, line: &str| {
        let error = decode_employee(&with_line(&good, key, line)).unwrap_err();
        (error.field, error.reason)
    };
    let is = |field: &str, reason: &str| (field.to_owned(), reason.to_owned());
    assert_eq!(
        refused("status", "status: \"paused\"\n"),
        is("status", "only an agent is paused")
    );
    assert_eq!(
        refused("offboarded_at", "offboarded_at: \"2026-11-14T10:00:00Z\"\n"),
        is("offboarded_at", "set only when inactive")
    );
    assert_eq!(
        refused(
            "reports_to_employee_id",
            "reports_to_employee_id: \"01900000-0000-7000-8000-000000000009\"\n"
        ),
        is("reports_to_employee_id", "nobody reports to themselves")
    );
    assert_eq!(
        refused("handle", "handle: \"here\"\n"),
        is("handle", "not a handle")
    );
    assert_eq!(
        refused("handle", "handle: \"Ana\"\n"),
        is("handle", "not a handle")
    );
    assert_eq!(
        refused("may_hire", "may_hire: 1\n"),
        is("may_hire", "not true or false")
    );
    assert!(decode_employee(b"handle: \"ana\"\n").is_err());

    let entry = |kind: &str, url: &str, id: u64, key: &str| {
        format!(
            "{{\"created_at\":null,\"created_by_id\":null,\"created_by_kind\":\"system\",\
             \"id\":\"01900000-0000-7000-8000-{id:012x}\",\"key\":{key},\"kind\":\"{kind}\",\
             \"remote_url\":{url},\"revision\":1,\"schema_version\":1,\"updated_at\":null,\
             \"updated_by_id\":null,\"updated_by_kind\":\"system\"}}"
        )
    };
    let file = |entries: &[String]| {
        format!("repositories: [{}]\nschema_version: 1\n", entries.join(",")).into_bytes()
    };
    let refused = |entries: &[String]| {
        let error = decode_repositories(&file(entries)).unwrap_err();
        (error.field, error.reason)
    };
    let url = "\"https://example.test/a.git\"";
    assert!(decode_repositories(&file(&[entry("remote", url, 1, "\"a\"")])).is_ok());
    assert_eq!(
        refused(&[entry("remote", "null", 1, "null")]),
        is(
            "repositories[0].remote_url",
            "a remote has a URL and a local has none"
        )
    );
    assert_eq!(
        refused(&[entry("local", url, 1, "null")]).0,
        "repositories[0].remote_url"
    );
    assert_eq!(
        refused(&[
            entry("local", "null", 1, "null"),
            entry("local", "null", 1, "null")
        ]),
        is("repositories[1].id", "listed twice")
    );
    assert_eq!(
        refused(&[
            entry("local", "null", 1, "\"a\""),
            entry("local", "null", 2, "\"a\"")
        ]),
        is("repositories[1].key", "listed twice")
    );
    assert!(decode_repositories(&file(&[
        entry("local", "null", 1, "null"),
        entry("local", "null", 2, "null")
    ]))
    .is_ok());

    let code = |code: &str, retired: &str| {
        format!(
            "{{\"code\":\"{code}\",\"issued_at\":\"2026-11-14T10:00:00Z\",\
             \"office_id\":\"01900000-0000-7000-8000-000000000002\",\"retired_at\":{retired}}}"
        )
    };
    let refused = |entries: &[String]| {
        let text = format!("codes: [{}]\nschema_version: 1\n", entries.join(","));
        let error = decode_office_codes(text.as_bytes()).unwrap_err();
        (error.field, error.reason)
    };
    for bad in ["I", "O", "a", "ABC", "", "A1"] {
        assert_eq!(
            refused(&[code(bad, "null")]),
            is("codes[0].code", "not an office code"),
            "{bad}"
        );
    }
    assert_eq!(
        refused(&[code("A", "null"), code("A", "null")]),
        is("codes[1].code", "issued twice")
    );
    assert_eq!(
        refused(&[code("A", "null"), code("B", "null")]),
        is("codes[1].office_id", "has two current codes")
    );

    let counter = b"entity_kind: \"project\"\nlast_number: 1\nscope_id: \"01900000-0000-7000-8000-000000000001\"\nscope_kind: \"organization\"\n";
    assert_eq!(
        decode_team_counter(counter).unwrap_err().field,
        "entity_kind"
    );
}

#[test]
fn a_path_names_its_record_and_nothing_else_is_this_familys() {
    let (org, a, b, e) = (id(1), id(0xa), id(0xb), id(0xe));
    assert_eq!(organization_path().as_str(), "state/organization.yaml");
    assert_eq!(kind_of(&organization_path()), Some(Kind::Organization));
    assert_eq!(kind_of(&repositories_path()), Some(Kind::Repositories));
    assert_eq!(kind_of(&office_codes_path()), Some(Kind::OfficeCodes));
    assert_eq!(
        office_path(&a).as_str(),
        "state/offices/01900000-0000-7000-8000-00000000000a.yaml"
    );
    assert_eq!(kind_of(&office_path(&a)), Some(Kind::Office(a.clone())));
    assert_eq!(
        team_counter_path(&org).as_str(),
        "state/identity/counters/organization/01900000-0000-7000-8000-000000000001/team.yaml"
    );
    assert_eq!(
        kind_of(&team_counter_path(&org)),
        Some(Kind::TeamCounter(org.clone()))
    );
    assert_eq!(
        team_allocation_path(&org, 12).as_str(),
        "state/identity/allocations/organization/01900000-0000-7000-8000-000000000001/team/12.yaml"
    );
    assert_eq!(
        kind_of(&team_allocation_path(&org, 12)),
        Some(Kind::TeamAllocation(org.clone(), 12))
    );

    let role = role_path(&a);
    assert_eq!(
        role.as_str(),
        "state/roles/01900000-0000-7000-8000-00000000000a.md"
    );
    assert_eq!(kind_of(&role), Some(Kind::Role(a.clone())));

    let chain = vec![a.clone(), b.clone()];
    let nested = team_path(&chain).unwrap();
    assert_eq!(
        nested.as_str(),
        "state/teams/01900000-0000-7000-8000-00000000000a/teams/01900000-0000-7000-8000-00000000000b/team.yaml"
    );
    assert_eq!(kind_of(&nested), Some(Kind::Team(chain.clone())));
    let top = team_path(&chain[..1]).unwrap();
    assert_eq!(kind_of(&top), Some(Kind::Team(vec![a.clone()])));
    for kind in [EmployeeKind::Person, EmployeeKind::Agent] {
        let profile = employee_path(&chain, kind, &e).unwrap();
        assert!(profile
            .as_str()
            .ends_with(&format!("/{}/{e}/profile.md", kind.folder())));
        assert_eq!(
            kind_of(&profile),
            Some(Kind::Employee(chain.clone(), kind, e.clone()))
        );
    }
    assert!(team_path(&[]).is_err());
    assert!(employee_path(&[], EmployeeKind::Agent, &e).is_err());
    // Too deep for a path.
    let deep: Vec<Id> = (0..12).map(id).collect();
    assert!(team_path(&deep[..10]).is_ok());
    assert!(team_path(&deep).is_err());

    for other in [
        "state",
        "state/organization.yml",
        "state/offices/hq.yaml",
        "state/offices/01900000-0000-7000-8000-00000000000a.yaml/x",
        "state/roles/not-an-id.md",
        "state/roles/01900000-0000-7000-8000-00000000000a.yaml",
        "state/teams/team.yaml",
        "state/teams/not-an-id/team.yaml",
        "state/teams/01900000-0000-7000-8000-00000000000a/work/tasks/x.md",
        "state/teams/01900000-0000-7000-8000-00000000000a/agents/01900000-0000-7000-8000-00000000000e/unknown.md",
        "state/teams/01900000-0000-7000-8000-00000000000a/hires/01900000-0000-7000-8000-00000000000e/profile.md",
        "state/teams/01900000-0000-7000-8000-00000000000a/teams/team.yaml",
        "state/identity/counters/organization/01900000-0000-7000-8000-000000000001/project.yaml",
        "state/identity/counters/project/01900000-0000-7000-8000-000000000001/team.yaml",
        "state/identity/allocations/organization/01900000-0000-7000-8000-000000000001/team/0.yaml",
        "state/identity/allocations/organization/01900000-0000-7000-8000-000000000001/team/01.yaml",
        "state/identity/allocations/organization/01900000-0000-7000-8000-000000000001/team/x.yaml",
        "local/organization.yaml",
        "recovery/commands/01900000-0000-7000-8000-00000000000a.json",
    ] {
        let path = DurablePath::parse(other).unwrap();
        assert_eq!(kind_of(&path), None, "{other}");
    }
}

#[test]
fn the_text_rules_are_the_contracts() {
    assert!(is_stable_id("01900000-0000-7000-8000-00000000000a"));
    for bad in [
        "",
        "01900000-0000-7000-8000-00000000000A",
        "01900000-0000-4000-8000-00000000000a",
        "01900000-0000-7000-7000-00000000000a",
        "0190000000007000800000000000000a",
    ] {
        assert!(!is_stable_id(bad), "{bad}");
    }
    assert!(is_line("Zoë's Org") && is_line("a"));
    for bad in ["", " a", "a ", "a\nb", "a\u{2028}b", &"a".repeat(201)] {
        assert!(!is_line(bad), "{bad:?}");
    }
    assert!(is_line(&"é".repeat(200)));
    assert!(is_slug("a") && is_slug("example-org-2") && is_slug(&"a".repeat(64)));
    for bad in ["", "-a", "a-", "a--b", "A", "a_b", "a b", &"a".repeat(65)] {
        assert!(!is_slug(bad), "{bad:?}");
    }
    for good in ["ana", "a_b-2", "7up", "hr", "ana.silva", "a.b.c", "a.-_.b"] {
        assert!(is_handle(good), "{good}");
    }
    assert!(is_handle(&"a".repeat(MAX_HANDLE_BYTES)));
    assert_eq!(MAX_HANDLE_BYTES, 64);
    for bad in [
        "",
        "here",
        "owner",
        "office",
        "Ana",
        "-a",
        "_a",
        ".a",
        "a.",
        "a..b",
        ".",
        "a b",
        "@ana",
        "run:1",
        // The shape of a member id, which messaging addresses by.
        &"a".repeat(26),
        &"a".repeat(MAX_HANDLE_BYTES + 1),
    ] {
        assert!(!is_handle(bad), "{bad:?}");
    }
    // The office's wider check of a messaging name shares the characters and the length.
    for shared in ["Ana", "a.", "a..b", "here"] {
        assert!(has_handle_characters(shared), "{shared}");
    }
    for bad in ["", ".a", "a b", "a:b", &"a".repeat(MAX_HANDLE_BYTES + 1)] {
        assert!(!has_handle_characters(bad), "{bad:?}");
    }
    assert!(is_branch("org/state") && !is_branch("org state") && !is_branch(""));
    assert!(is_remote_url("https://example.test/org.git"));
    assert!(!is_remote_url("") && !is_remote_url("https://a b") && !is_remote_url("a\n"));
    assert!(!is_remote_url(&"a".repeat(2049)));
    assert_eq!(time_text(Timestamp(0)), "1970-01-01T00:00:00.000Z");
}

#[test]
fn a_team_file_may_repeat_its_key_and_then_it_must_be_the_numbers() {
    let good = encode_team(&team());
    assert!(!String::from_utf8(good.clone()).unwrap().contains("key:"));
    let with_key = |key: &str| {
        let mut bytes = format!("key: {key}\n").into_bytes();
        bytes.extend(&good);
        bytes
    };
    assert_eq!(decode_team(&with_key("\"TEAM-1\"")), Ok(team()));
    for bad in [
        "\"TEAM-2\"",
        "\"team-1\"",
        "\"TEAM-01\"",
        "\"1\"",
        "null",
        "1",
    ] {
        let error = decode_team(&with_key(bad)).unwrap_err();
        assert_eq!(error.field, "key", "{bad}");
    }
    // Read and dropped: the key is never written back.
    let read = decode_team(&with_key("\"TEAM-1\"")).unwrap();
    assert_eq!(encode_team(&read), good);
}

#[test]
fn a_time_reads_at_second_or_millisecond_precision_and_is_written_in_milliseconds() {
    // Document 06 writes "2026-11-14T10:00:00Z"; this office writes milliseconds.
    let allocation = |at: &str| TeamAllocation {
        organization_id: id(1),
        number: 1,
        team_id: id(4),
        allocated_at: at.into(),
    };
    for at in ["2026-11-14T10:00:00Z", "2026-11-14T10:00:00.000Z"] {
        let bytes = encode_team_allocation(&allocation(at));
        assert_eq!(decode_team_allocation(&bytes), Ok(allocation(at)), "{at}");
        let stamped = with_line(
            &encode_team(&team()),
            "created_at",
            &format!("created_at: \"{at}\"\n"),
        );
        let read = decode_team(&stamped).unwrap();
        assert_eq!(read.meta.created_at.as_deref(), Some(at));
    }
    for bad in [
        "2026-11-14T10:00Z",
        "2026-11-14 10:00:00Z",
        "2026-11-14T10:00:00",
    ] {
        let bytes = encode_team_allocation(&allocation(bad));
        assert_eq!(
            decode_team_allocation(&bytes).unwrap_err().field,
            "allocated_at"
        );
    }
    assert_eq!(
        time_text(Timestamp(1_794_650_400_000)),
        "2026-11-14T10:00:00.000Z"
    );
}

fn execution(effort: Option<ReasoningEffort>) -> AgentExecutionConfig {
    AgentExecutionConfig {
        employee_id: id(7),
        model_profile_id: id(8),
        model_id: "opus".into(),
        tools_config: vec!["Read".into()],
        permission_preset: PermissionPreset::WorkspaceEdits,
        effort,
        enabled: true,
        meta: meta(),
        system_prompt_markdown: "You review code.\n".into(),
    }
}

#[test]
fn the_effort_levels_are_the_wire_contracts() {
    let names = ReasoningEffort::ALL.map(ReasoningEffort::as_str);
    assert_eq!(names, org_proto::hiring::AGENT_EFFORT_LEVELS);
    for level in ReasoningEffort::ALL {
        assert_eq!(ReasoningEffort::parse(level.as_str()), Some(level));
        let json = serde_json::to_value(level).unwrap();
        assert_eq!(json, serde_json::json!(level.as_str()));
    }
    for other in ["", "none", "High", "XHIGH", "extra", " low", "max "] {
        assert_eq!(ReasoningEffort::parse(other), None, "{other:?}");
    }
}

#[test]
fn an_execution_file_keeps_its_effort_and_one_without_reads_as_before() {
    for level in ReasoningEffort::ALL {
        let record = execution(Some(level));
        let bytes = encode_agent_execution(&record);
        let text = std::str::from_utf8(&bytes).unwrap();
        let line = format!("\neffort: \"{}\"\n", level.as_str());
        assert!(text.contains(&line), "{text}");
        assert_eq!(decode_agent_execution(&bytes).unwrap(), record);
    }

    // No effort: no key at all, so a file written before efforts existed is byte-for-byte
    // what this build writes, and reads back the same.
    let record = execution(None);
    let bytes = encode_agent_execution(&record);
    assert!(!std::str::from_utf8(&bytes).unwrap().contains("effort"));
    assert_eq!(decode_agent_execution(&bytes).unwrap(), record);

    // An explicit `null` is the runtime's default too.
    let with_xhigh = encode_agent_execution(&execution(Some(ReasoningEffort::Xhigh)));
    let null = with_line(&with_xhigh, "effort", "effort: null\n");
    assert_eq!(decode_agent_execution(&null).unwrap(), record);
}

#[test]
fn an_execution_file_with_an_unknown_effort_is_refused_by_field() {
    let bytes = encode_agent_execution(&execution(Some(ReasoningEffort::High)));
    for line in [
        "effort: turbo\n",
        "effort: \"\"\n",
        "effort: none\n",
        "effort: High\n",
        "effort: 3\n",
        "effort: [high]\n",
    ] {
        let err = decode_agent_execution(&with_line(&bytes, "effort", line)).unwrap_err();
        assert_eq!(err.field, "effort", "{line}: {err:?}");
    }
}
