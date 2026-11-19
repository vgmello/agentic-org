//! How names are compared: sibling teams, and a name a reader looks up.

use org_proto::OrganizationName;

/// The key names are compared by (ignoring case and edges) for a name already stored.
/// A stored name always parses; the fallback only keeps a damaged row from panicking.
pub fn name_key(stored: &str) -> String {
    OrganizationName::parse(stored)
        .map(|n| n.key().to_owned())
        .unwrap_or_else(|_| stored.trim().to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::name_key;

    #[test]
    fn names_compare_ignoring_case_and_edges() {
        assert_eq!(name_key("Platform"), name_key("platform"));
        assert_ne!(name_key("Platform"), name_key("Platforms"));
    }
}
