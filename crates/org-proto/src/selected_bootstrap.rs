//! Nonsecret selected HQ bootstrap input. Paths and authority are never supplied here.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanV1 {
    pub version: u32,
    pub name: String,
    pub slug: String,
    pub repository_remote_url: String,
    pub state_branch: String,
}

/// The person record of the account that creates the organization. The host fills it
/// from the account; the top-level team is named after the organization and led by it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreatorV1 {
    /// UUIDv7. The same id the host links the account to (its owner member).
    pub employee_id: String,
    pub name: String,
    pub handle: String,
}

/// The built-in HR lead's handle. No other bootstrap employee may hold it.
pub const HR_HANDLE: &str = "hr";

/// Handles no employee may hold: `here` addresses a channel's members, and `owner` and
/// `office` are addresses of the office itself.
const RESERVED_HANDLES: [&str; 3] = ["here", "owner", "office"];

/// The longest handle, in bytes.
pub const HANDLE_MAX: usize = 64;

/// Whether `handle` is a stored employee handle: lowercase ASCII letters, digits, dots,
/// underscores or hyphens, starting and ending with a letter or digit, not reserved and
/// not shaped like a member id or a run address.
pub fn valid_handle(handle: &str) -> bool {
    let edge = |b: Option<&u8>| b.is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit());
    let bytes = handle.as_bytes();
    bytes.len() <= HANDLE_MAX
        && edge(bytes.first())
        && edge(bytes.last())
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"._-".contains(b))
        && !RESERVED_HANDLES.contains(&handle)
        && !crate::is_shaped_like_an_address(handle)
}

impl CreatorV1 {
    /// The creator of an organization from the account's username: the name is the
    /// username as registered, the handle is the username made a valid handle. The
    /// mapping is deterministic, so a retried creation derives the same person.
    pub fn from_login(employee_id: String, login: &str) -> Self {
        let mut handle = String::new();
        for c in login.trim().chars().flat_map(char::to_lowercase) {
            let c = if c.is_ascii_lowercase() || c.is_ascii_digit() || "._-".contains(c) {
                c
            } else {
                '-'
            };
            handle.push(c);
        }
        let trimmed = handle.trim_matches(|c: char| !c.is_ascii_alphanumeric());
        let mut handle: String = trimmed.chars().take(HANDLE_MAX - 2).collect();
        while handle.ends_with(|c: char| !c.is_ascii_alphanumeric()) {
            handle.pop();
        }
        if handle.is_empty() {
            handle.push_str("person");
        }
        if handle == HR_HANDLE || !valid_handle(&handle) {
            handle.push_str("-1");
        }
        let name: String = login
            .trim()
            .chars()
            .filter(|c| !c.is_control())
            .take(NAME_MAX_CHARS)
            .collect();
        Self {
            employee_id,
            name: if name.trim().is_empty() {
                handle.clone()
            } else {
                name.trim().into()
            },
            handle,
        }
    }
    fn valid(&self) -> bool {
        crate::is_uuid_v7(&self.employee_id)
            && !self.name.is_empty()
            && self.name.trim() == self.name
            && self.name.chars().count() <= NAME_MAX_CHARS
            && !self.name.chars().any(char::is_control)
            && valid_handle(&self.handle)
            && self.handle != HR_HANDLE
    }
}
const NAME_MAX_CHARS: usize = 255;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreparedV1 {
    pub plan: PlanV1,
    pub organization_id: String,
    pub office_id: String,
    pub repository_id: String,
    pub home_team_id: String,
    pub hr_employee_id: String,
    pub creator: CreatorV1,
}

impl PlanV1 {
    /// Initial admitted remote profile: HTTPS or SSH URL, no credential-bearing
    /// query/fragment/password. Validation is local syntax only, never a probe.
    pub fn valid(&self) -> bool {
        self.version == 1
            && normalized_name(&self.name)
            && valid_slug(&self.slug)
            && remote(&self.repository_remote_url)
            && branch(&self.state_branch)
    }
}
impl PlanV1 {
    /// The first field that breaks a rule, with the rule: `unsupported` for a version this
    /// build doesn't know, `remote_required` for a repository that isn't an HTTPS or SSH
    /// remote, `format` otherwise. `None` exactly when [`PlanV1::valid`].
    pub fn invalid_field(&self) -> Option<(&'static str, &'static str)> {
        let url = &self.repository_remote_url;
        if self.version != 1 {
            Some(("version", "unsupported"))
        } else if !normalized_name(&self.name) {
            Some(("name", "format"))
        } else if !valid_slug(&self.slug) {
            Some(("slug", "format"))
        } else if !url.starts_with("https://") && !url.starts_with("ssh://") {
            Some(("repository_remote_url", "remote_required"))
        } else if !remote(url) {
            Some(("repository_remote_url", "format"))
        } else if !branch(&self.state_branch) {
            Some(("state_branch", "format"))
        } else {
            None
        }
    }
}
impl PreparedV1 {
    pub fn valid(&self) -> bool {
        let ids = [
            &self.organization_id,
            &self.office_id,
            &self.repository_id,
            &self.home_team_id,
            &self.hr_employee_id,
            &self.creator.employee_id,
        ];
        self.plan.valid()
            && self.creator.valid()
            && ids.iter().all(|id| crate::is_uuid_v7(id))
            && ids.iter().enumerate().all(|(i, id)| !ids[..i].contains(id))
    }
}
fn normalized_name(name: &str) -> bool {
    crate::OrganizationName::parse(name).is_ok_and(|n| n.as_str() == name)
}
fn valid_slug(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && !s.starts_with('-')
        && !s.ends_with('-')
        && !s.contains("--")
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}
fn remote(s: &str) -> bool {
    if s.len() > 2048
        || s.chars().any(|c| c.is_whitespace() || c.is_control())
        || s.contains(['?', '#', '\\'])
    {
        return false;
    }
    let (rest, ssh) = if let Some(r) = s.strip_prefix("https://") {
        (r, false)
    } else if let Some(r) = s.strip_prefix("ssh://") {
        (r, true)
    } else {
        return false;
    };
    let Some((authority, path)) = rest.split_once('/') else {
        return false;
    };
    if path.is_empty() || path.starts_with('/') {
        return false;
    }
    let authority = if ssh {
        let Some(host) = ssh_host(authority) else {
            return false;
        };
        host
    } else {
        authority
    };
    remote_host(authority)
}

fn ssh_host(authority: &str) -> Option<&str> {
    match authority.split_once('@') {
        Some((user, host))
            if !user.is_empty()
                && user
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c)) =>
        {
            Some(host)
        }
        Some(_) => None,
        None => Some(authority),
    }
}

fn remote_host(authority: &str) -> bool {
    let (host, port) = authority
        .split_once(':')
        .map_or((authority, None), |(h, p)| (h, Some(p)));
    !host.is_empty()
        && !host.starts_with('.')
        && !host.ends_with('.')
        && host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".-".contains(&b))
        && port.is_none_or(|p| {
            !p.is_empty()
                && p.bytes().all(|b| b.is_ascii_digit())
                && p.parse::<u16>().is_ok_and(|n| n != 0)
        })
}
fn branch(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 255
        && s != "@"
        && !s.starts_with('-')
        && !s.ends_with('.')
        && !s.contains([' ', '~', '^', ':', '?', '*', '[', '\\'])
        && !s.chars().any(char::is_control)
        && !s.contains("..")
        && !s.contains("@{")
        && s.split('/')
            .all(|p| !p.is_empty() && !p.starts_with('.') && !p.ends_with(".lock"))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn plan() -> PlanV1 {
        PlanV1 {
            version: 1,
            name: "Example".into(),
            slug: "example".into(),
            repository_remote_url: "ssh://git@example.test/org.git".into(),
            state_branch: "org/state".into(),
        }
    }
    fn prepared() -> PreparedV1 {
        let id = |n: u32| format!("01900000-0000-7000-8000-{n:012x}");
        PreparedV1 {
            plan: plan(),
            organization_id: id(1),
            office_id: id(2),
            repository_id: id(3),
            home_team_id: id(4),
            hr_employee_id: id(5),
            creator: CreatorV1::from_login(id(6), "Ana"),
        }
    }
    #[test]
    fn the_plan_has_no_team_name_and_refuses_an_unknown_field() {
        let mut v = serde_json::to_value(plan()).unwrap();
        assert!(v.get("home_team_name").is_none());
        v["home_team_name"] = "Core".into();
        assert!(serde_json::from_value::<PlanV1>(v).is_err());
    }
    #[test]
    fn the_creator_is_a_distinct_uuid_v7_person_with_a_valid_handle_that_is_not_hr() {
        assert!(prepared().valid());
        let mut p = prepared();
        p.creator.employee_id = "01ARZ3NDEKTSV4RRFFQ69G5FAV".into();
        assert!(!p.valid(), "a 26-character id is not a creator id");
        let mut p = prepared();
        p.creator.employee_id = p.hr_employee_id.clone();
        assert!(!p.valid(), "the creator is not the HR lead");
        for handle in [
            "hr", "here", "owner", "office", "", "Ana", "-ana", "ana.", "a b", "run:1",
        ] {
            let mut p = prepared();
            p.creator.handle = handle.into();
            assert!(!p.valid(), "{handle:?}");
        }
        for name in ["", " Ana", "Ana\n"] {
            let mut p = prepared();
            p.creator.name = name.into();
            assert!(!p.valid(), "{name:?}");
        }
        let mut v = serde_json::to_value(prepared()).unwrap();
        v.as_object_mut().unwrap().remove("creator");
        assert!(serde_json::from_value::<PreparedV1>(v).is_err());
    }
    #[test]
    fn a_username_maps_to_one_valid_handle_and_keeps_its_name() {
        let id = || "01900000-0000-7000-8000-000000000006".to_string();
        for (login, name, handle) in [
            ("Ana", "Ana", "ana"),
            ("  Ana Souza ", "Ana Souza", "ana-souza"),
            ("HR", "HR", "hr-1"),
            ("here", "here", "here-1"),
            ("owner", "owner", "owner-1"),
            ("ana@example.test", "ana@example.test", "ana-example.test"),
            ("_.ana._", "_.ana._", "ana"),
            ("\u{e9}", "\u{e9}", "person"),
            (
                "01ARZ3NDEKTSV4RRFFQ69G5FAV",
                "01ARZ3NDEKTSV4RRFFQ69G5FAV",
                "01arz3ndektsv4rrffq69g5fav-1",
            ),
        ] {
            let c = CreatorV1::from_login(id(), login);
            assert_eq!(
                (c.name.as_str(), c.handle.as_str()),
                (name, handle),
                "{login:?}"
            );
            assert!(c.valid(), "{login:?}");
            assert_eq!(c, CreatorV1::from_login(id(), login), "deterministic");
        }
        // Any username at all yields a valid creator.
        let long = "x".repeat(300);
        for login in [
            "",
            "   ",
            "---",
            "\u{0}",
            long.as_str(),
            "run:7",
            "a\u{212a}",
            "\u{130}",
        ] {
            assert!(CreatorV1::from_login(id(), login).valid(), "{login:?}");
        }
    }
    #[test]
    fn an_invalid_plan_names_its_first_field_and_rule() {
        assert_eq!(plan().invalid_field(), None);
        type Damage = fn(&mut PlanV1);
        let cases: [(Damage, (&str, &str)); 6] = [
            (|p| p.version = 2, ("version", "unsupported")),
            (|p| p.name = " Example".into(), ("name", "format")),
            (|p| p.slug = "Example".into(), ("slug", "format")),
            (
                |p| p.repository_remote_url = "file:///tmp/org".into(),
                ("repository_remote_url", "remote_required"),
            ),
            (
                |p| p.repository_remote_url = "https://example.test/org?token=x".into(),
                ("repository_remote_url", "format"),
            ),
            (
                |p| p.state_branch = "../state".into(),
                ("state_branch", "format"),
            ),
        ];
        for (damage, expected) in cases {
            let mut p = plan();
            damage(&mut p);
            assert_eq!(p.invalid_field(), Some(expected));
            assert!(!p.valid());
        }
    }
    #[test]
    fn explicit_local_validation_keeps_remote_policy() {
        assert!(plan().valid());
        for url in [
            "file:///tmp/org",
            "https://user:password@example.test/org",
            "https://example.test/org?token=x",
            "ssh://git@example.test:0/org",
            "https://../org",
            "https://example.test/",
        ] {
            let mut p = plan();
            p.repository_remote_url = url.into();
            assert!(!p.valid(), "{url}");
        }
        for b in [
            "",
            "../state",
            "-option",
            "state.lock",
            "refs//state",
            "@{x}",
            "org\\state",
        ] {
            let mut p = plan();
            p.state_branch = b.into();
            assert!(!p.valid(), "{b}");
        }
        let mut p = plan();
        p.slug = "Example".into();
        assert!(!p.valid());
    }
}
