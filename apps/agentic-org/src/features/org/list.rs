//! `org list`: the organizations, and those still being created.

use clap::Args;
use org_cli_shell::context::ExecutionContext;
use serde_json::{json, Value};

use super::shown_org;
use crate::args::{InstanceArgs, LoginArgs};
use crate::error::CliError;
use crate::view::{self, column, Column, Show};
use crate::{completers, selection};

#[derive(Args, Debug)]
pub(super) struct ListArgs {
    #[command(flatten)]
    pub(super) instance: InstanceArgs,
    #[command(flatten)]
    pub(super) login: LoginArgs,
}

const COLUMNS: [Column; 6] = [
    column("ID", "id", Show::Id),
    column("SLUG", "slug", Show::Plain),
    column("NAME", "name", Show::Name),
    column("STATE", "state", Show::Status),
    column("HQ OFFICE", "hq_office_id", Show::Dim),
    column("OFFICES", "offices", Show::Count),
];

pub(super) fn run(args: ListArgs, ctx: &mut ExecutionContext) -> Result<(), CliError> {
    let target = selection::select(args.instance.name.as_deref(), None)?;
    let listed = org_host_setup::org_list(target.instance(), args.login.login.as_deref())?;
    let json = view::json(&listed)?;
    let mut rows: Vec<Value> = view::items(&json, "organizations")
        .iter()
        .map(|org| {
            let mut shown = shown_org(org);
            shown["state"] = Value::from("active");
            shown
        })
        .collect();
    rows.extend(view::items(&json, "creating").iter().map(|pending| {
        json!({
            "id": pending["operation_id"],
            "slug": pending["slug"],
            "name": pending["name"],
            "state": "creating",
            "offices": [],
        })
    }));
    completers::remember(
        target.instance(),
        completers::ORGS,
        view::candidates(&json, "organizations", "id", "slug"),
    );
    view::list(ctx, &rows, &json, "id", &COLUMNS, "No organizations.")
}
