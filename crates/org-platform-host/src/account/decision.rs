//! Whether a group is a user's private group, pure: the rule over the two account
//! database entries. The GNU/Linux probe supplies them (`linux.rs`).
//!
//! The rule cannot see an account that has the group as its *primary* group: no member
//! list names it. The module documentation says what follows from that.

/// A user's entry in the account database.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct UserEntry {
    /// The user's name.
    pub(super) name: Vec<u8>,
    /// The user's primary group.
    pub(super) primary_group: u32,
}

/// A group's entry in the account database.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct GroupEntry {
    /// The group's id.
    pub(super) id: u32,
    /// The group's name.
    pub(super) name: Vec<u8>,
    /// The names in the group's member list. Accounts that have the group as their
    /// primary group are not in it.
    pub(super) members: Vec<Vec<u8>>,
}

/// Whether `group` is the private group of `user`: the user's primary group, named as
/// the user, whose member list is empty or names only the user. Not detected: another
/// account with this group as its primary group.
pub(super) fn is_private_group(user: &UserEntry, group: &GroupEntry) -> bool {
    !user.name.is_empty()
        && group.id == user.primary_group
        && group.name == user.name
        && group.members.iter().all(|member| *member == user.name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(name: &str, primary_group: u32) -> UserEntry {
        UserEntry {
            name: name.as_bytes().to_vec(),
            primary_group,
        }
    }

    fn group(id: u32, name: &str, members: &[&str]) -> GroupEntry {
        GroupEntry {
            id,
            name: name.as_bytes().to_vec(),
            members: members.iter().map(|m| m.as_bytes().to_vec()).collect(),
        }
    }

    #[test]
    fn a_private_group_is_the_users_primary_group_named_as_the_user_with_no_other_member() {
        let alex = user("alex", 1000);
        // No member list, and one that names only the user.
        assert!(is_private_group(&alex, &group(1000, "alex", &[])));
        assert!(is_private_group(&alex, &group(1000, "alex", &["alex"])));
        assert!(is_private_group(
            &alex,
            &group(1000, "alex", &["alex", "alex"])
        ));
        // Root's own group is root's private group by the same rule.
        assert!(is_private_group(&user("root", 0), &group(0, "root", &[])));
    }

    #[test]
    fn another_member_another_name_or_another_group_is_not_private() {
        let alex = user("alex", 1000);
        for (case, entry) in [
            ("another member", group(1000, "alex", &["sam"])),
            (
                "the user and another",
                group(1000, "alex", &["alex", "sam"]),
            ),
            ("a shared group", group(1000, "staff", &[])),
            ("a name that differs by case", group(1000, "Alex", &[])),
            ("a name that only starts alike", group(1000, "alex2", &[])),
            ("not the primary group", group(1001, "alex", &[])),
            ("an empty member name", group(1000, "alex", &[""])),
        ] {
            assert!(!is_private_group(&alex, &entry), "{case}");
        }
        // A supplementary group the user is alone in is not the primary group.
        assert!(!is_private_group(&alex, &group(27, "sudo", &["alex"])));
        // An account without a name has no private group.
        assert!(!is_private_group(&user("", 1000), &group(1000, "", &[])));
    }
}
