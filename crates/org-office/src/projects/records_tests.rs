//! `project.yaml` and its `TASK-88` default definition of done.

use org_store::{Id, Timestamp};

use super::*;
use crate::organization::records::By;

fn id(n: u64) -> Id {
    Id::parse(&format!("01900000-0000-7000-8000-{n:012x}")).unwrap()
}

fn project(default_dod: Option<Vec<String>>) -> Project {
    Project {
        id: id(1),
        number: 3,
        slug: "billing".into(),
        name: "Billing".into(),
        owner_team_id: id(2),
        archived: false,
        development_process_id: None,
        contributing_team_ids: vec![id(4)],
        repositories: Vec::new(),
        default_dod,
        meta: Meta::created(&By::Employee(id(9)), Timestamp(1_700_000_000_000)),
    }
}

#[test]
fn a_projects_default_definition_of_done_inherits_opts_out_or_overrides() {
    let inherits = encode_project(&project(None));
    assert!(!String::from_utf8_lossy(&inherits).contains("default_dod"));
    assert_eq!(decode_project(&inherits), Ok(project(None)));
    for default_dod in [Some(vec![]), Some(vec!["Deployed to staging".to_owned()])] {
        let record = project(default_dod);
        let bytes = encode_project(&record);
        assert_eq!(decode_project(&bytes), Ok(record.clone()));
        assert_eq!(record.to_wire().default_dod, record.default_dod);
    }
    let text = String::from_utf8(inherits).unwrap();
    for bad in ["[\" untrimmed\"]", "[1]", "null"] {
        let broken = format!("{text}default_dod: {bad}\n");
        assert!(decode_project(broken.as_bytes()).is_err(), "{bad}");
    }
}
