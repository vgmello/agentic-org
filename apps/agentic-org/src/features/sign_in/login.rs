//! `agentic-org sign-in [-- <command...>]`.

use org_cli_shell::context::ExecutionContext;
use serde_json::Value;

use super::office_proxy;
use super::SignInArgs;
use crate::error::CliError;
use crate::selection;
use crate::view::{self, column, Column, Show};

const FIELDS: [Column; 3] = [
    column("account", "account", Show::Name),
    column("saved_office_id", "saved_context.office_id", Show::Id),
    column("saved_instance", "saved_context.instance_name", Show::Plain),
];

pub(crate) fn run(args: SignInArgs, ctx: &mut ExecutionContext) -> Result<(), CliError> {
    let target = selection::select(args.instance.name.as_deref(), args.office.as_deref())?;
    let (instance, office) = (target.explicit_instance(), target.explicit_office());
    if !args.command.is_empty() {
        // The table reads the line before the host is reached: a line it refuses (exit 2)
        // never touches an instance or an office.
        return office_proxy::sign_in_and_execute(args.command, instance, office, ctx);
    }
    let mut prompt = super::approval_link_stream();
    let signed = org_host_setup::sign_in_only(instance, office, &mut prompt)?;
    let json = view::json(&signed)?;
    view::detail(ctx, &json, &json, "account", &FIELDS)?;
    if json["saved_context"] == Value::Null {
        ctx.success("Signed in.");
    } else {
        ctx.success("Signed in; this office is now the default for office commands.");
    }
    Ok(())
}
