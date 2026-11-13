use super::*;

const MEMBER: &str = "01JD8Z3F0000000000000000CD";

fn project() -> ProjectKey {
    ProjectKey::new("atlas").unwrap()
}

fn team() -> Place {
    Place::Team(TeamId::new("web").unwrap())
}

#[test]
fn paths_map_to_their_place_and_nothing_else_does() {
    let key = project();
    let member = MemberId::new(MEMBER).unwrap();
    let cases = [
        (
            Repo::Handbook,
            "processes/review.md",
            Some(Place::Organization),
        ),
        (
            Repo::Handbook,
            "teams/web/processes/review.md",
            Some(team()),
        ),
        (
            Repo::Handbook,
            &format!("agents/{MEMBER}/processes/mine.md"),
            Some(Place::Agent(member)),
        ),
        (
            Repo::Project(&key),
            "docs/processes/review.md",
            Some(Place::Project(key.clone())),
        ),
        (Repo::Handbook, "processes/sub/review.md", None),
        (Repo::Handbook, "processes/review.txt", None),
        (Repo::Handbook, "processes/.md", None),
        (Repo::Handbook, "processes/.hidden.md", None),
        (Repo::Handbook, "processes/../processes/review.md", None),
        (Repo::Handbook, "/processes/review.md", None),
        (Repo::Handbook, "processes//review.md", None),
        (Repo::Handbook, "teams/w eb/processes/review.md", None),
        (Repo::Handbook, "agents/short/processes/mine.md", None),
        (Repo::Handbook, "docs/processes/review.md", None),
        (Repo::Project(&key), "processes/review.md", None),
        (Repo::Project(&key), "docs/processes/x/review.md", None),
    ];
    for (repo, path, expected) in cases {
        assert_eq!(location(repo, path), expected, "{path}");
    }
}

#[test]
fn ids_and_keys_have_their_shapes() {
    assert!(ProcessKey::new("review-and-merge").is_some() && ProcessKey::new("1x").is_some());
    for bad in ["", "-x", "Review", "a_b", &"a".repeat(65)] {
        assert!(ProcessKey::new(bad).is_none(), "{bad}");
    }
    assert!(ProcessKey::new(&"a".repeat(64)).is_some());
    assert!(
        TeamId::new("Web_team-1").is_some()
            && TeamId::new("").is_none()
            && TeamId::new("a/b").is_none()
    );
    assert!(
        ProjectKey::new("ab").is_some()
            && ProjectKey::new("a").is_none()
            && ProjectKey::new("Ab").is_none()
    );
    assert!(MemberId::new(MEMBER).is_some() && MemberId::new(&MEMBER[1..]).is_none());
    for good in ["ab", "a1", "abcdefghij"] {
        assert!(ProjectKey::new(good).is_some(), "{good}");
    }
    for bad in ["a", "Ab", "1a", "abcdefghijk", "a-b"] {
        assert!(ProjectKey::new(bad).is_none(), "{bad}");
    }
    assert!(TeamId::new(&"t".repeat(64)).is_some() && TeamId::new(&"t".repeat(65)).is_none());
    assert!(MemberId::new(&format!("{}-", &MEMBER[1..])).is_none());
    for level in [
        Level::Organization,
        Level::Team,
        Level::Project,
        Level::Agent,
    ] {
        assert_eq!(Level::parse(level.as_str()), Some(level));
    }
    assert_eq!(Level::parse("Team"), None);
}
