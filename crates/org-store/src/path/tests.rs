use super::*;

fn path(text: &str) -> DurablePath {
    DurablePath::parse(text).unwrap()
}

#[test]
fn a_durable_path_is_relative_normal_and_under_a_known_tree() {
    for good in [
        "state",
        "state/organization.yaml",
        "recovery/commands/01900000-0000-7000-8000-000000000001.json",
        "local/offices/a/probe/x",
        "state/teams/a_b-c.d/0.yaml",
    ] {
        assert_eq!(path(good).as_str(), good);
    }
    let long = "a".repeat(256);
    for bad in [
        "",
        "/state/a",
        "state/../authority/a",
        "state/./a",
        "state//a",
        "state/a/",
        "runtime/a",
        "State/a",
        // Only the portable set: nothing a file system folds, normalizes or refuses.
        "state/Teams/a",
        "state/teams/ünïcode/team.yaml",
        "state/a b",
        "state/a:b",
        "state/a*",
        "state/a\nb",
        "state/a\0b",
        "state/a\\b",
        "../state/a",
        "state/.store-tmp-1-1",
        &format!("state/{long}"),
    ] {
        assert!(DurablePath::parse(bad).is_err(), "accepted {bad:?}");
    }
    assert!(DurablePath::parse(&format!("state/{}", "a".repeat(255))).is_ok());
    assert_eq!(path("state/a/b").parent(), Some(path("state/a")));
    assert_eq!(path("state").parent(), None);
    let deep = format!("state/{}", "abcdefg/".repeat(128));
    let max = DurablePath::MAX_BYTES;
    assert_eq!(max, 1024);
    assert!(DurablePath::parse(&deep[..max]).is_ok());
    assert!(DurablePath::parse(&format!("{}x", &deep[..max])).is_err());
}

#[test]
fn the_deepest_team_task_comment_path_fits_with_headroom() {
    // `MAX_TEAM_DEPTH = 16` team chain (the organization's limit), then a task's
    // comment: the longest path the data contract names.
    let uuid = |n: u64| format!("01900000-0000-7000-8000-{n:012x}");
    let mut folder = String::from("state");
    for depth in 0..16 {
        folder.push_str("/teams/");
        folder.push_str(&uuid(depth));
    }
    let comment = format!(
        "{folder}/work/tasks/{}/comments/{}.md",
        uuid(0x100),
        uuid(0x200)
    );
    assert_eq!(comment.len(), 790);
    assert!(DurablePath::parse(&comment).is_ok());
    // Over 200 bytes (more than a fifth of the limit) are left for anything longer the
    // contract adds.
    assert!(
        comment.len() + 200 <= DurablePath::MAX_BYTES,
        "{}",
        comment.len()
    );
    for name in ["task.md", "checklist.yaml", "operations.yaml"] {
        let file = format!("{folder}/work/tasks/{}/{name}", uuid(0x100));
        assert!(DurablePath::parse(&file).is_ok(), "{name}");
    }
    // The history of a task is not under its team: its path is short at any depth.
    let history = format!("state/work-history/tasks/{}/{}.json", uuid(0x100), i64::MAX);
    assert!(history.len() < 128);
    assert!(DurablePath::parse(&history).is_ok());
}

#[test]
fn paths_order_by_bytes_and_prefixes_match_whole_components() {
    let teams = path("state/teams");
    assert!(path("state/teams/a/team.yaml").is_under(&teams));
    assert!(path("state/teams").is_under(&teams));
    assert!(!path("state/teams-old/a").is_under(&teams));
    assert!(!path("state").is_under(&teams));
    assert_eq!(teams.join("a").unwrap(), path("state/teams/a"));
    assert!(teams.join("a/b").is_err());
    assert!(teams.join("..").is_err());
    assert_eq!(path("local/a").tree(), Tree::Local);
    assert!(!path("state").names_a_file());
    assert!(path("state/a").names_a_file());
    let mut sorted = [path("state/b"), path("state/a/z"), path("state/a-")];
    sorted.sort();
    let texts: Vec<_> = sorted.iter().map(DurablePath::as_str).collect();
    assert_eq!(texts, ["state/a-", "state/a/z", "state/b"]);
    assert_eq!(
        serde_json::from_str::<DurablePath>(r#""state/a""#).unwrap(),
        path("state/a")
    );
    assert!(serde_json::from_str::<DurablePath>(r#""state/../a""#).is_err());
}

#[test]
fn a_durable_path_serializes_as_its_text_and_deserializes_only_when_valid() {
    assert_eq!(
        serde_json::to_string(&path("state/a")).unwrap(),
        r#""state/a""#
    );
    let refused = serde_json::from_str::<DurablePath>(r#""runtime/a""#).unwrap_err();
    assert_eq!(
        refused.to_string(),
        "invalid: a durable path starts with a known tree"
    );
    assert!(serde_json::from_str::<DurablePath>("7").is_err());
}

#[test]
fn a_component_is_portable_and_never_a_temporary_name() {
    for good in ["a", "a.b", "a_b-c", "0", "...", &"a".repeat(255)] {
        assert!(component_ok(good), "refused {good:?}");
    }
    for bad in [
        "",
        ".",
        "..",
        "A",
        "a/b",
        "a b",
        "é",
        &format!("{TEMP_PREFIX}1"),
        &"a".repeat(256),
    ] {
        assert!(!component_ok(bad), "accepted {bad:?}");
    }
}
