//! Organization-owned membership, structure, people, directory and status policies.
//! Persistence remains in the private store; root facades compose it with these capabilities.
mod apply;
pub mod directory;
mod people;
mod policy;
mod status;
mod structure;
mod view;

pub use apply::{apply, UnknownChange};
pub use people::owner_bootstrap;
pub use policy::name_key;
pub use status::presence_after_status;
pub use view::{OrgView, OrgWrite};

pub(crate) type Decision = Result<Vec<org_proto::org::OrgChange>, org_proto::ApiError>;
pub(crate) use people::{
    ending_relations, member_add_person, member_remove, member_set_reports_to, member_update_person,
};
pub(crate) use policy::{
    check_handle_free, check_id, check_member_id, check_text, check_version, fail, found,
    parse_name, refuse_offboarded,
};
pub(crate) use status::check as check_status;
pub(crate) use structure::{
    member_move, project_add_team, project_remove_team, team_add_member, team_create, team_delete,
    team_move, team_remove_member, team_rename, team_set_lead, team_set_title,
};
