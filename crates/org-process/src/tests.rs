use std::collections::{BTreeMap, BTreeSet};

use crate::{
    find_builtin_skill, list_builtin_skills, render_instructions, resolve_single_chain,
    validate_write, CallerKind, DocumentKind, DocumentStatus, LayerOutcome, ProcessKey,
    RenderError, RenderInput, Scope, SingleChainInputs, StructureDocInput, WinnerKind,
    WriteCheckInput, WriteViolation,
};

fn doc(
    id: &str,
    key: Option<&str>,
    kind: DocumentKind,
    scope: Scope,
    overridable: bool,
    status: DocumentStatus,
    body: &str,
) -> StructureDocInput {
    StructureDocInput {
        id: id.to_owned(),
        key: key.map(|k| ProcessKey::new(k).expect("valid key")),
        kind,
        title: format!("Title {id}"),
        status,
        owner_employee_id: (status == DocumentStatus::Current).then(|| "emp-owner".to_owned()),
        scope,
        overridable,
        revision: 1,
        body_markdown: body.to_owned(),
        path: format!("processes/{id}.md"),
        blob_sha256: format!("sha-{id}"),
        invalid_field: None,
    }
}

#[test]
fn single_chain_resolution_applies_org_team_project_and_enforces_non_overridable() {
    let org_sealed = doc(
        "d-org-sec",
        Some("security-baseline"),
        DocumentKind::Standard,
        Scope::Organization,
        false,
        DocumentStatus::Current,
        "Never leak secrets.",
    );
    let team_illegal_override = doc(
        "d-team-sec",
        Some("security-baseline"),
        DocumentKind::Standard,
        Scope::Team("t-child".into()),
        true,
        DocumentStatus::Current,
        "Attempted weaker security.",
    );
    let org_review = doc(
        "d-org-rev",
        Some("code-review"),
        DocumentKind::Process,
        Scope::Organization,
        true,
        DocumentStatus::Current,
        "One reviewer.",
    );
    let root_team_review = doc(
        "d-troot-rev",
        Some("code-review"),
        DocumentKind::Process,
        Scope::Team("t-root".into()),
        true,
        DocumentStatus::Current,
        "Two reviewers.",
    );
    let proj_review = doc(
        "d-proj-rev",
        Some("code-review"),
        DocumentKind::Process,
        Scope::Project("p-1".into()),
        true,
        DocumentStatus::Current,
        "Two reviewers plus CI.",
    );
    // Contributing team document must NOT apply (outside accountable chain).
    let contrib_team_doc = doc(
        "d-contrib",
        Some("contrib-only"),
        DocumentKind::Process,
        Scope::Team("t-contrib".into()),
        true,
        DocumentStatus::Current,
        "Should be ignored.",
    );
    // Draft and deprecated docs are excluded silently.
    let draft_doc = doc(
        "d-draft",
        Some("wip"),
        DocumentKind::Process,
        Scope::Organization,
        true,
        DocumentStatus::Draft,
        "Draft text.",
    );
    let dep_doc = doc(
        "d-dep",
        Some("old"),
        DocumentKind::Standard,
        Scope::Organization,
        true,
        DocumentStatus::Deprecated,
        "Old text.",
    );

    let set = resolve_single_chain(&SingleChainInputs {
        team_chain: vec!["t-root".into(), "t-child".into()],
        project_id: Some("p-1".into()),
        documents: vec![
            org_sealed,
            team_illegal_override,
            org_review,
            root_team_review,
            proj_review,
            contrib_team_doc,
            draft_doc,
            dep_doc,
        ],
        scope_names: BTreeMap::from([
            (Scope::Team("t-root".into()), "Engineering".into()),
            (Scope::Team("t-child".into()), "Payments".into()),
            (Scope::Project("p-1".into()), "Billing".into()),
        ]),
    });

    // Security baseline stays at Organization; team attempt is Refused.
    let sec = set.find_by_key("security-baseline").expect("sec entry");
    assert_eq!(sec.winner.as_ref().unwrap().doc.id, "d-org-sec");
    assert_eq!(sec.winner.as_ref().unwrap().kind, WinnerKind::Inherited);
    assert!(sec
        .layers
        .iter()
        .any(|l| l.doc.id == "d-team-sec" && matches!(l.outcome, LayerOutcome::Refused { .. })));

    // Overridable code-review resolves to project winner replacing root team.
    let rev = set.find_by_key("code-review").expect("rev entry");
    let rev_w = rev.winner.as_ref().unwrap();
    assert_eq!(rev_w.doc.id, "d-proj-rev");
    assert_eq!(rev_w.kind, WinnerKind::Override);
    assert_eq!(rev_w.replaces.as_ref().unwrap().id, "d-troot-rev");

    // Contributing team, draft, and deprecated docs do not appear.
    assert!(set.find_by_key("contrib-only").is_none());
    assert!(set.find_by_key("wip").is_none());
    assert!(set.find_by_key("old").is_none());
}

#[test]
fn clashed_or_invalid_non_overridable_doc_keeps_key_sealed_below() {
    let mut bad_org = doc(
        "d-bad-org",
        Some("pci"),
        DocumentKind::Process,
        Scope::Organization,
        false,
        DocumentStatus::Current,
        "Broken front matter",
    );
    bad_org.invalid_field = Some("owner_employee_id".into());
    let team_try = doc(
        "d-team-pci",
        Some("pci"),
        DocumentKind::Process,
        Scope::Team("t1".into()),
        true,
        DocumentStatus::Current,
        "Lower override attempt",
    );
    let set = resolve_single_chain(&SingleChainInputs {
        team_chain: vec!["t1".into()],
        project_id: None,
        documents: vec![bad_org, team_try],
        scope_names: BTreeMap::new(),
    });
    let pci = set.find_by_key("pci").unwrap();
    assert!(pci.winner.is_none());
    assert!(pci
        .layers
        .iter()
        .any(|l| l.doc.id == "d-team-pci" && matches!(l.outcome, LayerOutcome::Refused { .. })));
}

#[test]
fn render_delivers_mandatory_in_full_refuses_overflow_and_caps_overridable() {
    let mandatory = doc(
        "d-mand",
        Some("pci-policy"),
        DocumentKind::Standard,
        Scope::Organization,
        false,
        DocumentStatus::Current,
        "MANDATORY RULE: Encrypt cardholder data at rest.",
    );
    let big_overridable = doc(
        "d-over",
        Some("style-guide"),
        DocumentKind::Process,
        Scope::Organization,
        true,
        DocumentStatus::Current,
        &"x".repeat(30 * 1024),
    );
    let set = resolve_single_chain(&SingleChainInputs {
        team_chain: vec![],
        project_id: None,
        documents: vec![mandatory, big_overridable],
        scope_names: BTreeMap::new(),
    });

    // Normal budget: mandatory included in full; oversized overridable omitted (not marked delivered).
    let rendered = render_instructions(&set, &RenderInput::default()).unwrap();
    assert!(rendered.text.contains("MANDATORY RULE"));
    assert!(rendered.text.contains("Process context limit reached"));
    assert_eq!(rendered.mandatory_count, 1);
    assert_eq!(rendered.omitted_overridable_count, 1);
    assert_eq!(rendered.delivered_blobs.len(), 1);
    assert_eq!(rendered.delivered_blobs[0].0, "pci-policy");

    // Tiny budget where mandatory alone overflows -> launch refused naming the document.
    let err = render_instructions(
        &set,
        &RenderInput {
            read_blobs: BTreeMap::new(),
            overridable_cap_bytes: 1024,
            total_budget_bytes: 64,
        },
    )
    .unwrap_err();
    match err {
        RenderError::MandatoryBudgetExceeded {
            document_titles, ..
        } => assert_eq!(document_titles, vec!["Title d-mand".to_owned()]),
    }

    // Already-read blobs are skipped in body section.
    let small_over = doc(
        "d-small",
        Some("small-guide"),
        DocumentKind::Process,
        Scope::Organization,
        true,
        DocumentStatus::Current,
        "Short body text.",
    );
    let set2 = resolve_single_chain(&SingleChainInputs {
        team_chain: vec![],
        project_id: None,
        documents: vec![small_over],
        scope_names: BTreeMap::new(),
    });
    let mut read_blobs = BTreeMap::new();
    read_blobs.insert("small-guide".into(), BTreeSet::from(["sha-d-small".into()]));
    let r2 = render_instructions(
        &set2,
        &RenderInput {
            read_blobs,
            ..RenderInput::default()
        },
    )
    .unwrap();
    assert!(r2.delivered_blobs.is_empty());
    assert!(!r2.text.contains("Short body text.\n```"));
}

#[test]
fn write_validation_enforces_already_exists_sealed_and_agent_boundaries() {
    let existing = vec![
        doc(
            "d-org-sealed",
            Some("sec"),
            DocumentKind::Standard,
            Scope::Organization,
            false,
            DocumentStatus::Current,
            "Sealed",
        ),
        doc(
            "d-org-over",
            Some("style"),
            DocumentKind::Standard,
            Scope::Organization,
            true,
            DocumentStatus::Current,
            "Org style",
        ),
        doc(
            "d-proj-1",
            Some("api-conventions"),
            DocumentKind::Standard,
            Scope::Project("p1".into()),
            true,
            DocumentStatus::Current,
            "Project API",
        ),
    ];

    // Duplicate key at same scope -> AlreadyExists.
    let dup = validate_write(&WriteCheckInput {
        caller: CallerKind::Person { is_owner: true },
        target_scope: Scope::Project("p1".into()),
        existing_document_id: None,
        key: Some("api-conventions"),
        kind: DocumentKind::Standard,
        title: "Dup",
        status: DocumentStatus::Current,
        owner_employee_id: Some("emp-1"),
        overridable: true,
        body_markdown: "body",
        project_owner_team_id: Some("t-owner"),
        ancestor_team_chain: &["t-root".into(), "t-owner".into()],
        existing_docs: &existing,
    });
    assert_eq!(
        dup,
        Err(WriteViolation::AlreadyExists {
            key: "api-conventions".into()
        })
    );

    // Overriding a sealed org key at project scope -> Sealed.
    let sealed = validate_write(&WriteCheckInput {
        caller: CallerKind::Person { is_owner: true },
        target_scope: Scope::Project("p1".into()),
        existing_document_id: None,
        key: Some("sec"),
        kind: DocumentKind::Standard,
        title: "Try override",
        status: DocumentStatus::Current,
        owner_employee_id: Some("emp-1"),
        overridable: true,
        body_markdown: "body",
        project_owner_team_id: Some("t-owner"),
        ancestor_team_chain: &["t-root".into(), "t-owner".into()],
        existing_docs: &existing,
    });
    assert_eq!(
        sealed,
        Err(WriteViolation::Sealed {
            key: "sec".into(),
            sealed_by_scope: Scope::Organization
        })
    );

    // Agent of owning team's descendant can add a new overridable project standard.
    let agent_ok = validate_write(&WriteCheckInput {
        caller: CallerKind::Agent {
            home_team_id: "t-sub".into(),
            home_team_chain: vec!["t-root".into(), "t-owner".into(), "t-sub".into()],
        },
        target_scope: Scope::Project("p1".into()),
        existing_document_id: None,
        key: Some("grpc-errors"),
        kind: DocumentKind::Standard,
        title: "gRPC errors",
        status: DocumentStatus::Current,
        owner_employee_id: Some("emp-agent"),
        overridable: true,
        body_markdown: "Use canonical error codes.",
        project_owner_team_id: Some("t-owner"),
        ancestor_team_chain: &["t-root".into(), "t-owner".into()],
        existing_docs: &existing,
    });
    assert!(agent_ok.is_ok());

    // Agent CANNOT replace an inherited org/team key, CANNOT write non-overridable,
    // CANNOT write at team/org scope, and CANNOT write from a contributing team.
    let agent_inherited = validate_write(&WriteCheckInput {
        caller: CallerKind::Agent {
            home_team_id: "t-owner".into(),
            home_team_chain: vec!["t-root".into(), "t-owner".into()],
        },
        target_scope: Scope::Project("p1".into()),
        existing_document_id: None,
        key: Some("style"),
        kind: DocumentKind::Standard,
        title: "Override org style",
        status: DocumentStatus::Current,
        owner_employee_id: Some("emp-agent"),
        overridable: true,
        body_markdown: "nope",
        project_owner_team_id: Some("t-owner"),
        ancestor_team_chain: &["t-root".into(), "t-owner".into()],
        existing_docs: &existing,
    });
    assert!(matches!(
        agent_inherited,
        Err(WriteViolation::NotAuthorized { .. })
    ));

    let agent_contrib = validate_write(&WriteCheckInput {
        caller: CallerKind::Agent {
            home_team_id: "t-contrib".into(),
            home_team_chain: vec!["t-root".into(), "t-contrib".into()],
        },
        target_scope: Scope::Project("p1".into()),
        existing_document_id: None,
        key: Some("new-proj-std"),
        kind: DocumentKind::Standard,
        title: "From contrib",
        status: DocumentStatus::Current,
        owner_employee_id: Some("emp-agent"),
        overridable: true,
        body_markdown: "nope",
        project_owner_team_id: Some("t-owner"),
        ancestor_team_chain: &["t-root".into(), "t-owner".into()],
        existing_docs: &existing,
    });
    assert!(matches!(
        agent_contrib,
        Err(WriteViolation::NotAuthorized { .. })
    ));

    // Non-owner person CANNOT unseal or deprecate an existing non-overridable org standard.
    let non_owner_unseal = validate_write(&WriteCheckInput {
        caller: CallerKind::Person { is_owner: false },
        target_scope: Scope::Organization,
        existing_document_id: Some("d-org-sealed"),
        key: Some("sec"),
        kind: DocumentKind::Standard,
        title: "Weakened Org Security",
        status: DocumentStatus::Deprecated,
        owner_employee_id: Some("emp-person"),
        overridable: true,
        body_markdown: "trying to unseal",
        project_owner_team_id: None,
        ancestor_team_chain: &[],
        existing_docs: &existing,
    });
    assert!(matches!(
        non_owner_unseal,
        Err(WriteViolation::NotAuthorized { .. })
    ));

    // Agent CANNOT unseal or modify an existing non-overridable project standard.
    let mut with_locked_proj = existing.clone();
    with_locked_proj.push(doc(
        "d-proj-locked",
        Some("pci-lock"),
        DocumentKind::Standard,
        Scope::Project("p1".into()),
        false,
        DocumentStatus::Current,
        "Strict",
    ));
    let agent_unseal_proj = validate_write(&WriteCheckInput {
        caller: CallerKind::Agent {
            home_team_id: "t-owner".into(),
            home_team_chain: vec!["t-root".into(), "t-owner".into()],
        },
        target_scope: Scope::Project("p1".into()),
        existing_document_id: Some("d-proj-locked"),
        key: Some("pci-lock"),
        kind: DocumentKind::Standard,
        title: "PCI Lock Unlocked",
        status: DocumentStatus::Current,
        owner_employee_id: Some("emp-agent"),
        overridable: true,
        body_markdown: "unlocked",
        project_owner_team_id: Some("t-owner"),
        ancestor_team_chain: &["t-root".into(), "t-owner".into()],
        existing_docs: &with_locked_proj,
    });
    assert!(matches!(
        agent_unseal_proj,
        Err(WriteViolation::NotAuthorized { .. })
    ));
}

#[test]
fn render_budget_respects_separator_unique_omitted_count_and_preserves_notice() {
    let mut docs = vec![doc(
        "d-mand",
        Some("mand"),
        DocumentKind::Standard,
        Scope::Organization,
        false,
        DocumentStatus::Current,
        "Always check auth.",
    )];
    for i in 0..8 {
        docs.push(doc(
            &format!("d-ovr-{i}"),
            Some(&format!("guide-{i}")),
            DocumentKind::Process,
            Scope::Organization,
            true,
            DocumentStatus::Current,
            &"Body text for guide. ".repeat(20),
        ));
    }
    let set = resolve_single_chain(&SingleChainInputs {
        team_chain: vec![],
        project_id: None,
        documents: docs,
        scope_names: BTreeMap::new(),
    });
    // Tight budget: enough for mandatory + separator + header + notice + a couple summaries,
    // forcing both summary and body omission.
    let mand_only = render_instructions(
        &set,
        &RenderInput {
            read_blobs: BTreeMap::new(),
            overridable_cap_bytes: 0,
            total_budget_bytes: 64 * 1024,
        },
    )
    .expect("mandatory only");
    let tight_budget = mand_only.text.len() + 2 + 320;
    let rendered = render_instructions(
        &set,
        &RenderInput {
            read_blobs: BTreeMap::new(),
            overridable_cap_bytes: crate::OVERRIDABLE_CAP_BYTES,
            total_budget_bytes: tight_budget,
        },
    )
    .expect("tight render");
    assert!(
        rendered.text.len() <= tight_budget,
        "rendered {} bytes exceeded tight budget {}",
        rendered.text.len(),
        tight_budget
    );
    assert!(
        rendered.omitted_overridable_count <= 8 && rendered.omitted_overridable_count > 0,
        "omitted count must never double-count: {}",
        rendered.omitted_overridable_count
    );
    assert!(
        rendered.text.contains("Process context limit reached"),
        "omission notice must not be dropped: {}",
        rendered.text
    );
}

#[test]
fn builtin_skills_are_discoverable_and_readable() {
    let skills = list_builtin_skills();
    assert_eq!(skills.len(), 12);
    let skill = find_builtin_skill("processes-and-standards").expect("skill found");
    assert_eq!(skill.source, "built_in");
    assert!(skill.body_markdown.contains("agent-os process list"));
    assert!(find_builtin_skill("no-such-skill").is_none());
}
