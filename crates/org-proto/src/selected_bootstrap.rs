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
    pub home_team_name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreparedV1 {
    pub plan: PlanV1,
    pub organization_id: String,
    pub office_id: String,
    pub repository_id: String,
    pub home_team_id: String,
    pub hr_employee_id: String,
}

impl PlanV1 {
    /// Initial admitted remote profile: HTTPS or SSH URL, no credential-bearing
    /// query/fragment/password. Validation is local syntax only, never a probe.
    pub fn valid(&self) -> bool {
        self.version == 1
            && normalized_name(&self.name)
            && normalized_name(&self.home_team_name)
            && valid_slug(&self.slug)
            && remote(&self.repository_remote_url)
            && branch(&self.state_branch)
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
        ];
        self.plan.valid()
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
            home_team_name: "Core".into(),
        }
    }
    #[test]
    fn explicit_local_validation_keeps_remote_policy_and_home_team() {
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
        p.home_team_name.clear();
        assert!(!p.valid());
        let mut p = plan();
        p.slug = "Example".into();
        assert!(!p.valid());
    }
}
