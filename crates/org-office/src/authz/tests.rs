use std::cell::Cell;
use std::collections::BTreeMap;

use super::*;

const OWNER: &str = "owner";
const PERSON: &str = "person";
const OTHER_PERSON: &str = "other-person";
const AGENT: &str = "agent";
const OWNER_AGENT: &str = "owners-agent";
const TEAM: &str = "team-a";
const OTHER_TEAM: &str = "team-b";

#[derive(Default)]
struct Facts {
    by_key: BTreeMap<String, ParentFacts>,
    fail: Cell<bool>,
}

fn key(resource: &ResourceRef) -> String {
    format!("{resource:?}")
}

impl Facts {
    fn with(mut self, resource: ResourceRef, facts: ParentFacts) -> Self {
        self.by_key.insert(key(&resource), facts);
        self
    }
}

impl ResourceFacts for Facts {
    fn facts(&self, resource: &ResourceRef) -> Result<ParentFacts, OfficeError> {
        if self.fail.get() {
            return Err(OfficeError::Invalid("facts unavailable"));
        }
        Ok(self
            .by_key
            .get(&key(resource))
            .cloned()
            .unwrap_or(ParentFacts::Missing))
    }
}

fn principal(id: &str, kind: PrincipalKind, teams: &[&str]) -> PrincipalContext {
    PrincipalContext {
        employee_id: id.into(),
        kind,
        is_owner: id == OWNER,
        team_ids: teams.iter().map(|t| (*t).to_owned()).collect(),
        active: true,
    }
}

fn owner() -> PrincipalContext {
    principal(OWNER, PrincipalKind::Person, &[])
}
fn person() -> PrincipalContext {
    principal(PERSON, PrincipalKind::Person, &[TEAM])
}
fn other_person() -> PrincipalContext {
    principal(OTHER_PERSON, PrincipalKind::Person, &[OTHER_TEAM])
}
fn agent() -> PrincipalContext {
    principal(AGENT, PrincipalKind::Agent, &[TEAM])
}

fn members(ids: &[&str]) -> BTreeSet<String> {
    ids.iter().map(|id| (*id).to_owned()).collect()
}

fn facts() -> Facts {
    Facts::default()
        .with(ResourceRef::Organization, ParentFacts::Organization)
        .with(
            ResourceRef::Project("p1".into()),
            ParentFacts::Project {
                team_ids: members(&[TEAM]),
                archived: false,
            },
        )
        .with(
            ResourceRef::Project("archived".into()),
            ParentFacts::Project {
                team_ids: members(&[TEAM]),
                archived: true,
            },
        )
        .with(ResourceRef::Task("t1".into()), ParentFacts::Task)
        .with(
            ResourceRef::TaskComment {
                task_id: "t1".into(),
                comment_id: "c1".into(),
            },
            ParentFacts::TaskComment {
                author_id: PERSON.into(),
            },
        )
        .with(
            ResourceRef::Message("dm".into()),
            ParentFacts::Message {
                members: members(&[PERSON, AGENT]),
                sender_id: Some(AGENT.into()),
                open: false,
            },
        )
        .with(
            ResourceRef::Message("channel".into()),
            ParentFacts::Message {
                members: members(&[AGENT]),
                sender_id: Some(AGENT.into()),
                open: true,
            },
        )
        .with(
            ResourceRef::Conversation("dm".into()),
            ParentFacts::Conversation {
                members: members(&[PERSON, AGENT]),
                open: false,
            },
        )
        .with(
            ResourceRef::Conversation("channel".into()),
            ParentFacts::Conversation {
                members: members(&[AGENT]),
                open: true,
            },
        )
        .with(
            ResourceRef::AskAnswer {
                ask_message_id: "ask".into(),
                answerer_id: PERSON.into(),
            },
            ParentFacts::AskAnswer {
                members: members(&[PERSON, OTHER_PERSON]),
                open: false,
            },
        )
}

fn is_not_found(result: Result<(), Denied>) -> bool {
    matches!(result, Err(Denied::NotFound))
}

fn is_not_authorized(result: Result<(), Denied>) -> bool {
    matches!(result, Err(Denied::NotAuthorized(_)))
}

/// Refused with the hint that agents have no private space.
fn is_hint(result: Result<(), Denied>) -> bool {
    matches!(result, Err(Denied::NotAuthorized(why)) if why == AGENT_HAS_NO_PRIVATE_SPACE)
}

fn staged(by: &str) -> ResourceRef {
    ResourceRef::StagedUpload {
        id: "up".into(),
        uploaded_by: by.into(),
    }
}

fn private(owner: &str) -> ResourceRef {
    ResourceRef::UserPrivate {
        owner_employee_id: owner.into(),
    }
}

fn attachment(target: ResourceRef, by: &str) -> ResourceRef {
    ResourceRef::Attachment {
        id: "att".into(),
        target: Box::new(target),
        uploaded_by: by.into(),
    }
}

#[test]
fn a_staged_upload_is_the_uploaders_alone_and_others_get_not_found() {
    let facts = facts();
    let policy = AuthorizationPolicy::new(&facts);
    let upload = staged(PERSON);
    policy.can_read(&person(), &upload).unwrap();
    policy.can_write(&person(), &upload).unwrap();
    for intruder in [owner(), other_person(), agent()] {
        assert!(
            is_not_found(policy.can_read(&intruder, &upload)),
            "{intruder:?}"
        );
        assert!(is_not_found(policy.can_write(&intruder, &upload)));
        assert!(is_not_found(policy.can_attach(
            &intruder,
            &upload,
            &ResourceRef::Organization
        )));
    }
}

#[test]
fn the_private_space_admits_only_its_owning_person_and_never_an_agent() {
    let facts = facts();
    let policy = AuthorizationPolicy::new(&facts);
    let space = private(PERSON);
    policy.can_read(&person(), &space).unwrap();
    policy.can_write(&person(), &space).unwrap();
    // The owner, another person and the owner's own agent: refused as if it did not exist.
    let owners_agent = principal(OWNER_AGENT, PrincipalKind::Agent, &[]);
    for caller in [owner(), other_person(), agent(), owners_agent] {
        assert!(is_not_found(policy.can_read(&caller, &space)), "{caller:?}");
        assert!(
            is_not_found(policy.can_write(&caller, &space)),
            "{caller:?}"
        );
    }
    // An agent that shares the person's id (impossible in a directory, but the rule must not
    // hinge on ids alone) is refused too: told agents have no space, never let in.
    let impostor = principal(PERSON, PrincipalKind::Agent, &[TEAM]);
    assert!(is_hint(policy.can_read(&impostor, &space)));
    assert!(is_hint(policy.can_write(&impostor, &space)));
    let owners_space = private(OWNER);
    let owners_agent = principal(OWNER_AGENT, PrincipalKind::Agent, &[]);
    assert!(is_not_found(policy.can_read(&owners_agent, &owners_space)));
    let file = attachment(private(PERSON), PERSON);
    policy.can_read(&person(), &file).unwrap();
    assert!(is_not_found(policy.can_read(&agent(), &file)));
    assert!(is_not_found(policy.can_write(&owner(), &file)));
}

#[test]
fn an_inactive_caller_is_refused_everything() {
    let facts = facts();
    let policy = AuthorizationPolicy::new(&facts);
    let mut gone = person();
    gone.active = false;
    for resource in [
        staged(PERSON),
        private(PERSON),
        ResourceRef::Organization,
        ResourceRef::Project("p1".into()),
    ] {
        assert!(
            is_not_found(policy.can_read(&gone, &resource)),
            "{resource:?}"
        );
        assert!(policy.can_write(&gone, &resource).is_err());
    }
}

#[test]
fn attachments_inherit_their_parents_read_rule() {
    let facts = facts();
    let policy = AuthorizationPolicy::new(&facts);
    let on_dm = attachment(ResourceRef::Message("dm".into()), AGENT);
    policy.can_read(&person(), &on_dm).unwrap();
    policy.can_read(&agent(), &on_dm).unwrap();
    // The owner supervises every conversation, as `chat.messages` lets them (`TASK-98.2`).
    policy.can_read(&owner(), &on_dm).unwrap();
    // Not a member of the conversation: the file does not exist for them.
    assert!(is_not_found(policy.can_read(&other_person(), &on_dm)));
    // A channel's files are every employee's to read, as its messages are.
    let on_channel = attachment(ResourceRef::Message("channel".into()), AGENT);
    for caller in [owner(), person(), other_person(), agent()] {
        policy.can_read(&caller, &on_channel).unwrap();
    }

    let on_answer = attachment(
        ResourceRef::AskAnswer {
            ask_message_id: "ask".into(),
            answerer_id: PERSON.into(),
        },
        PERSON,
    );
    policy.can_read(&other_person(), &on_answer).unwrap();
    policy.can_read(&owner(), &on_answer).unwrap();
    assert!(is_not_found(policy.can_read(&agent(), &on_answer)));

    for parent in [
        ResourceRef::Organization,
        ResourceRef::Project("p1".into()),
        ResourceRef::Task("t1".into()),
    ] {
        let file = attachment(parent, PERSON);
        for caller in [owner(), person(), other_person(), agent()] {
            policy.can_read(&caller, &file).unwrap();
        }
    }
    // A parent that is gone, or a kind this office does not hold yet, hides the file.
    for parent in [
        ResourceRef::Project("gone".into()),
        ResourceRef::ReviewFinding("rf".into()),
        ResourceRef::ReviewFindingReply("rr".into()),
    ] {
        assert!(is_not_found(
            policy.can_read(&owner(), &attachment(parent, OWNER))
        ));
    }
}

#[test]
fn writing_a_parent_follows_its_owner_team_or_author() {
    let facts = facts();
    let policy = AuthorizationPolicy::new(&facts);
    // Phase 1: every active employee, agents included, publishes organization-wide.
    let org = ResourceRef::Organization;
    policy.can_write(&owner(), &org).unwrap();
    policy.can_write(&other_person(), &org).unwrap();
    policy.can_write(&agent(), &org).unwrap();
    let mut gone = agent();
    gone.active = false;
    assert!(is_not_found(policy.can_write(&gone, &org)));

    let project = ResourceRef::Project("p1".into());
    policy.can_write(&owner(), &project).unwrap();
    policy.can_write(&person(), &project).unwrap();
    policy.can_write(&agent(), &project).unwrap();
    assert!(is_not_authorized(
        policy.can_write(&other_person(), &project)
    ));
    assert!(is_not_authorized(
        policy.can_write(&owner(), &ResourceRef::Project("archived".into()))
    ));

    let comment = ResourceRef::TaskComment {
        task_id: "t1".into(),
        comment_id: "c1".into(),
    };
    policy.can_write(&person(), &comment).unwrap();
    assert!(is_not_authorized(policy.can_write(&owner(), &comment)));
    let dm = ResourceRef::Message("dm".into());
    policy.can_write(&agent(), &dm).unwrap();
    assert!(is_not_authorized(policy.can_write(&person(), &dm)));
    assert!(is_not_authorized(policy.can_write(&owner(), &dm)));
    assert!(is_not_found(policy.can_write(&other_person(), &dm)));
    policy
        .can_write(&agent(), &ResourceRef::Task("t1".into()))
        .unwrap();
}

#[test]
fn editing_an_attachment_needs_its_uploader_or_the_owner_and_the_parent() {
    let facts = facts();
    let policy = AuthorizationPolicy::new(&facts);
    let file = attachment(ResourceRef::Project("p1".into()), AGENT);
    policy.can_write(&agent(), &file).unwrap();
    policy.can_write(&owner(), &file).unwrap();
    assert!(is_not_authorized(policy.can_write(&person(), &file)));
    let archived = attachment(ResourceRef::Project("archived".into()), AGENT);
    assert!(is_not_authorized(policy.can_write(&agent(), &archived)));
}

#[test]
fn re_attaching_needs_read_on_the_source_and_write_on_the_target() {
    let facts = facts();
    let policy = AuthorizationPolicy::new(&facts);
    let private_file = attachment(private(PERSON), PERSON);
    let project = ResourceRef::Project("p1".into());
    // The person may share their own private file with a project they work on.
    policy
        .can_attach(&person(), &private_file, &project)
        .unwrap();
    // The owner may write the project but cannot read the person's private file.
    assert!(is_not_found(policy.can_attach(
        &owner(),
        &private_file,
        &project
    )));
    // A shared file may be re-attached only where the caller may write.
    let org_file = attachment(ResourceRef::Organization, OWNER);
    policy.can_attach(&agent(), &org_file, &project).unwrap();
    assert!(is_not_authorized(policy.can_attach(
        &other_person(),
        &org_file,
        &project
    )));
    // An agent can never place anything into a private space: its own is refused with the
    // hint to use a direct message, a person's as if it did not exist.
    assert!(is_hint(policy.can_attach(
        &agent(),
        &org_file,
        &private(AGENT)
    )));
    assert!(is_hint(policy.can_attach(
        &agent(),
        &staged(AGENT),
        &private(AGENT)
    )));
    assert!(is_not_found(policy.can_attach(
        &agent(),
        &org_file,
        &private(PERSON)
    )));
    // An agent may publish a staged upload of its own organization-wide.
    policy
        .can_attach(&agent(), &staged(AGENT), &ResourceRef::Organization)
        .unwrap();
    // A DM file cannot leave the conversation through someone outside it.
    let dm_file = attachment(ResourceRef::Message("dm".into()), AGENT);
    assert!(is_not_found(policy.can_attach(
        &other_person(),
        &dm_file,
        &ResourceRef::Organization
    )));
    // Nothing is attached to a file.
    assert!(is_not_authorized(policy.can_attach(
        &person(),
        &staged(PERSON),
        &org_file
    )));
    assert!(is_not_authorized(policy.can_attach(
        &person(),
        &project,
        &ResourceRef::Organization
    )));
}

#[test]
fn a_facts_failure_never_allows_and_surfaces_as_storage() {
    let facts = facts();
    facts.fail.set(true);
    let policy = AuthorizationPolicy::new(&facts);
    let denied = policy
        .can_read(&owner(), &attachment(ResourceRef::Organization, OWNER))
        .unwrap_err();
    assert!(matches!(denied, Denied::Storage(_)));
    assert!(denied.into_api().is_err());
    assert_eq!(
        Denied::NotFound.into_api().unwrap().code,
        ErrorCode::NotFound
    );
}

#[test]
fn files_go_into_a_conversation_only_with_a_member_of_it_or_the_owner() {
    let facts = facts();
    let policy = AuthorizationPolicy::new(&facts);
    let dm = ResourceRef::Conversation("dm".into());
    policy.can_attach(&agent(), &staged(AGENT), &dm).unwrap();
    policy.can_attach(&person(), &staged(PERSON), &dm).unwrap();
    policy.can_attach(&owner(), &staged(OWNER), &dm).unwrap();
    // Someone else's staged upload never exists for the caller, whoever they are.
    assert!(is_not_found(policy.can_attach(
        &person(),
        &staged(AGENT),
        &dm
    )));
    assert!(is_not_found(policy.can_attach(
        &owner(),
        &staged(AGENT),
        &dm
    )));
    // A stranger to a DM is not told it exists.
    assert!(is_not_found(policy.can_attach(
        &other_person(),
        &staged(OTHER_PERSON),
        &dm
    )));
    // Anyone reads a channel, but only its members post files in it.
    let channel = ResourceRef::Conversation("channel".into());
    policy
        .can_attach(&agent(), &staged(AGENT), &channel)
        .unwrap();
    assert!(is_not_authorized(policy.can_attach(
        &person(),
        &staged(PERSON),
        &channel
    )));
    assert!(is_not_found(policy.can_attach(
        &person(),
        &staged(PERSON),
        &ResourceRef::Conversation("gone".into())
    )));
}
