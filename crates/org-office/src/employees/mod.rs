//! Employee use cases. Organization owns membership, identity and placement; this module
//! consumes its contracts in one direction and owns agent profile/lifecycle transitions.
mod hire;
mod lifecycle;
mod policy;
mod update;

pub(crate) use hire::hire_employee;
pub(crate) use lifecycle::{member_offboard, member_set_agent_status, member_set_run_mode};
pub(crate) use update::member_update_agent;
