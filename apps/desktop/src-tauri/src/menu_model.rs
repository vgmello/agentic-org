//! What the menu-bar menu shows, as plain data: organizations A–Z, each with its offices
//! underneath (UX spec MENU-1 to MENU-3). `tray.rs` turns this into a native menu, so the rules
//! live here and can be tested without a window.

use crate::connections::local_office;
use org_proto::{OfficeInfo, OfficeRole, OfficeStatus, Organization};

/// The host name the app shows for the machine it runs on.
const THIS_MAC: &str = "This Mac";

/// More offices than this are collapsed into "N more offices…".
pub const MAX_OFFICES_SHOWN: usize = 5;
/// ⌘1 to ⌘9 pick the first nine organizations.
pub const MAX_SHORTCUTS: usize = 9;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OfficeLine {
    pub id: String,
    /// "HQ · This Mac — Online"
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OrganizationEntry {
    Available {
        id: String,
        name: String,
        current: bool,
        /// 1 to 9, in menu order.
        shortcut: Option<u8>,
        offices: Vec<OfficeLine>,
        /// Offices beyond [`MAX_OFFICES_SHOWN`].
        more_offices: usize,
    },
    /// Shown greyed with its reason on the next line; its offices aren't listed.
    Unavailable {
        id: String,
        name: String,
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Menu {
    /// The host answered: the organizations on this Mac (possibly none).
    Organizations(Vec<OrganizationEntry>),
    /// The host isn't running (the login service is off, or nothing is installed).
    OfficesNotRunning,
    /// The host is running but couldn't read its organizations.
    Unreadable,
}

/// What a menu item stands for. Item ids are built and read only here, so the format is tested.
/// The separator is a control character that can't appear in a ULID, so ids are never split wrongly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ItemId<'a> {
    Organization(&'a str),
    Office {
        organization: &'a str,
        office: &'a str,
    },
    Offices(&'a str),
}

const SEP: char = '\u{1f}';

pub fn organization_item(id: &str) -> String {
    format!("org{SEP}{id}")
}

pub fn office_item(organization: &str, office: &str) -> String {
    format!("office{SEP}{organization}{SEP}{office}")
}

pub fn offices_item(organization: &str) -> String {
    format!("offices{SEP}{organization}")
}

pub fn parse_item(id: &str) -> Option<ItemId<'_>> {
    let mut parts = id.split(SEP);
    match (parts.next()?, parts.next(), parts.next(), parts.next()) {
        ("org", Some(o), None, None) => Some(ItemId::Organization(o)),
        ("offices", Some(o), None, None) => Some(ItemId::Offices(o)),
        ("office", Some(o), Some(f), None) => Some(ItemId::Office {
            organization: o,
            office: f,
        }),
        _ => None,
    }
}

/// The status as the owner reads it (UX spec §3.1).
pub fn status_label(status: OfficeStatus) -> &'static str {
    match status {
        OfficeStatus::Active => "Online",
        OfficeStatus::Starting => "Starting",
        OfficeStatus::Pending => "Invited",
        OfficeStatus::Offline => "Offline",
        OfficeStatus::Unavailable => "Stopped",
        OfficeStatus::UpdateNeeded => "Update needed",
        _ => "Unknown",
    }
}

fn office_label(office: &OfficeInfo) -> String {
    let role = match office.role {
        OfficeRole::Hq => "HQ",
        _ => office.name.as_str(),
    };
    format!("{role} · {} — {}", office.host, status_label(office.status))
}

/// HQ first, then the other offices on this Mac, then the rest, each group A–Z by name.
fn ordered_offices(offices: &[OfficeInfo]) -> Vec<&OfficeInfo> {
    let mut sorted: Vec<&OfficeInfo> = offices.iter().collect();
    sorted.sort_by_key(|o| {
        (
            o.role != OfficeRole::Hq,
            o.host != THIS_MAC,
            o.name.to_lowercase(),
        )
    });
    sorted
}

/// An organization is unavailable when the office it is opened through here can't be used.
fn unavailable_reason(organization: &Organization) -> Option<&'static str> {
    let office = local_office(organization)?;
    (office.status == OfficeStatus::Unavailable).then_some("Its office on this Mac stopped")
}

pub fn build(organizations: &[Organization], current: Option<&str>) -> Menu {
    let mut sorted: Vec<&Organization> = organizations.iter().collect();
    sorted.sort_by_key(|o| o.name.to_lowercase());

    let mut shortcuts = 0u8;
    let entries = sorted
        .into_iter()
        .map(|organization| {
            if let Some(reason) = unavailable_reason(organization) {
                return OrganizationEntry::Unavailable {
                    id: organization.id.clone(),
                    name: organization.name.clone(),
                    reason: reason.to_string(),
                };
            }
            let shortcut = ((shortcuts as usize) < MAX_SHORTCUTS).then(|| {
                shortcuts += 1;
                shortcuts
            });
            let offices = ordered_offices(&organization.offices);
            OrganizationEntry::Available {
                id: organization.id.clone(),
                name: organization.name.clone(),
                current: current == Some(organization.id.as_str()),
                shortcut,
                more_offices: offices.len().saturating_sub(MAX_OFFICES_SHOWN),
                offices: offices
                    .into_iter()
                    .take(MAX_OFFICES_SHOWN)
                    .map(|o| OfficeLine {
                        id: o.id.clone(),
                        label: office_label(o),
                    })
                    .collect(),
            }
        })
        .collect();
    Menu::Organizations(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn office(id: &str, name: &str, role: OfficeRole, status: OfficeStatus) -> OfficeInfo {
        OfficeInfo {
            id: id.into(),
            organization_id: "o".into(),
            name: name.into(),
            role,
            host: "This Mac".into(),
            endpoint: None,
            status,
        }
    }

    fn org(id: &str, name: &str, offices: Vec<OfficeInfo>) -> Organization {
        Organization {
            id: id.into(),
            name: name.into(),
            created_at: "2026-09-14T09:30:00.000Z".into(),
            offices,
        }
    }

    fn entries(menu: Menu) -> Vec<OrganizationEntry> {
        match menu {
            Menu::Organizations(e) => e,
            _ => panic!("expected organizations"),
        }
    }

    #[test]
    fn menu_1_organizations_are_a_to_z_with_hq_first_and_a_status_on_every_office() {
        let orgs = vec![
            org(
                "z",
                "zeta",
                vec![office("z1", "HQ", OfficeRole::Hq, OfficeStatus::Active)],
            ),
            org(
                "a",
                "Alpha",
                vec![
                    office(
                        "a2",
                        "Studio Mac",
                        OfficeRole::Satellite,
                        OfficeStatus::Offline,
                    ),
                    office(
                        "a3",
                        "Build box",
                        OfficeRole::Satellite,
                        OfficeStatus::Starting,
                    ),
                    office("a1", "HQ", OfficeRole::Hq, OfficeStatus::Active),
                ],
            ),
        ];
        let e = entries(build(&orgs, None));
        let names: Vec<_> = e
            .iter()
            .map(|x| match x {
                OrganizationEntry::Available { name, .. } => name.clone(),
                OrganizationEntry::Unavailable { name, .. } => name.clone(),
            })
            .collect();
        assert_eq!(names, ["Alpha", "zeta"], "A–Z, ignoring case");
        let OrganizationEntry::Available { offices, .. } = &e[0] else {
            panic!()
        };
        let labels: Vec<_> = offices.iter().map(|o| o.label.as_str()).collect();
        assert_eq!(
            labels,
            [
                "HQ · This Mac — Online",
                "Build box · This Mac — Starting",
                "Studio Mac · This Mac — Offline"
            ]
        );
    }

    #[test]
    fn menu_2_the_current_organization_is_marked_and_shortcuts_follow_menu_order() {
        let orgs = vec![
            org(
                "b",
                "Beta",
                vec![office("b1", "HQ", OfficeRole::Hq, OfficeStatus::Active)],
            ),
            org(
                "a",
                "Alpha",
                vec![office("a1", "HQ", OfficeRole::Hq, OfficeStatus::Active)],
            ),
        ];
        let e = entries(build(&orgs, Some("b")));
        let flags: Vec<_> = e
            .iter()
            .map(|x| match x {
                OrganizationEntry::Available {
                    current, shortcut, ..
                } => (*current, *shortcut),
                _ => panic!(),
            })
            .collect();
        assert_eq!(flags, [(false, Some(1)), (true, Some(2))]);
    }

    #[test]
    fn menu_3_an_unavailable_organization_has_a_reason_no_offices_and_no_shortcut() {
        let orgs = vec![
            org(
                "d",
                "Down",
                vec![office(
                    "d1",
                    "HQ",
                    OfficeRole::Hq,
                    OfficeStatus::Unavailable,
                )],
            ),
            org(
                "u",
                "Up",
                vec![office("u1", "HQ", OfficeRole::Hq, OfficeStatus::Active)],
            ),
        ];
        let e = entries(build(&orgs, None));
        assert_eq!(
            e[0],
            OrganizationEntry::Unavailable {
                id: "d".into(),
                name: "Down".into(),
                reason: "Its office on this Mac stopped".into()
            }
        );
        let OrganizationEntry::Available { shortcut, .. } = &e[1] else {
            panic!()
        };
        assert_eq!(
            *shortcut,
            Some(1),
            "an unavailable organization doesn't use up a shortcut"
        );
    }

    #[test]
    fn more_than_five_offices_collapse_and_only_nine_shortcuts_are_given() {
        let many: Vec<_> = (0..8)
            .map(|i| {
                office(
                    &format!("o{i}"),
                    &format!("Office {i}"),
                    OfficeRole::Satellite,
                    OfficeStatus::Active,
                )
            })
            .collect();
        let e = entries(build(&[org("m", "Many", many)], None));
        let OrganizationEntry::Available {
            offices,
            more_offices,
            ..
        } = &e[0]
        else {
            panic!()
        };
        assert_eq!((offices.len(), *more_offices), (5, 3));

        let orgs: Vec<_> = (0..11)
            .map(|i| {
                org(
                    &format!("{i:02}"),
                    &format!("Org {i:02}"),
                    vec![office("x", "HQ", OfficeRole::Hq, OfficeStatus::Active)],
                )
            })
            .collect();
        let shortcuts: Vec<_> = entries(build(&orgs, None))
            .iter()
            .map(|x| match x {
                OrganizationEntry::Available { shortcut, .. } => *shortcut,
                _ => None,
            })
            .collect();
        assert_eq!(shortcuts.iter().flatten().count(), 9);
        assert_eq!(shortcuts[9], None);
    }

    #[test]
    fn no_organizations_is_an_empty_list_and_every_status_has_a_word() {
        assert_eq!(build(&[], None), Menu::Organizations(vec![]));
        for s in [
            OfficeStatus::Active,
            OfficeStatus::Starting,
            OfficeStatus::Pending,
            OfficeStatus::Offline,
            OfficeStatus::Unavailable,
            OfficeStatus::UpdateNeeded,
            OfficeStatus::Unknown,
        ] {
            assert!(!status_label(s).is_empty());
        }
    }

    #[test]
    fn item_ids_round_trip_even_when_ids_contain_colons() {
        let org = "org:with:colons";
        let office = "o:1";
        assert_eq!(
            parse_item(&organization_item(org)),
            Some(ItemId::Organization(org))
        );
        assert_eq!(parse_item(&offices_item(org)), Some(ItemId::Offices(org)));
        assert_eq!(
            parse_item(&office_item(org, office)),
            Some(ItemId::Office {
                organization: org,
                office
            })
        );
        for plain in [
            "open",
            "quit",
            "dock",
            "org",
            "org\u{1f}a\u{1f}b",
            "office\u{1f}a",
        ] {
            assert_eq!(parse_item(plain), None, "{plain:?}");
        }
    }

    #[test]
    fn an_organization_without_hq_is_judged_by_the_same_office_it_opens_through() {
        // Listed order: Zed first. `local_office` picks it (no HQ), so the menu must too.
        let orgs = vec![org(
            "x",
            "X",
            vec![
                office("z", "Zed", OfficeRole::Satellite, OfficeStatus::Unavailable),
                office("a", "Alpha", OfficeRole::Satellite, OfficeStatus::Active),
            ],
        )];
        assert!(matches!(
            entries(build(&orgs, None))[0],
            OrganizationEntry::Unavailable { .. }
        ));
    }

    #[test]
    fn offices_on_this_mac_come_before_remote_ones_after_hq() {
        let mut remote = office("r", "Aardvark", OfficeRole::Satellite, OfficeStatus::Active);
        remote.host = "hq-mac.local".into();
        let local = office("l", "Zebra", OfficeRole::Satellite, OfficeStatus::Active);
        let hq = office("h", "HQ", OfficeRole::Hq, OfficeStatus::Active);
        let e = entries(build(&[org("o", "O", vec![remote, local, hq])], None));
        let OrganizationEntry::Available { offices, .. } = &e[0] else {
            panic!()
        };
        let ids: Vec<_> = offices.iter().map(|o| o.id.as_str()).collect();
        assert_eq!(
            ids,
            ["h", "l", "r"],
            "HQ, then this Mac's, then the remote one"
        );
    }
}
