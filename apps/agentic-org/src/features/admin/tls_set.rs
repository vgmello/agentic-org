//! `admin tls set`.

use clap::Args;
use org_cli_shell::context::ExecutionContext;

use crate::args::{InstanceArgs, YesArgs};
use crate::error::CliError;
use org_cli_shell::prompt::require_confirmation;
use serde_json::json;

use crate::selection;
use crate::view::{self, column, Column, Show};

pub(super) const EXAMPLES: &str = "Examples:\n  agentic-org admin tls set --cert ./cert.pem --key ./key.pem --login owner\n  agentic-org admin tls set --cert ./cert.pem --key ./key.pem --login owner --dry-run";

#[derive(Args, Debug)]
pub(crate) struct SetArgs {
    /// The new certificate chain (PEM)
    #[arg(long, value_name = "PATH")]
    pub(super) cert: String,
    /// The new private key (PEM)
    #[arg(long, value_name = "PATH")]
    pub(super) key: String,
    /// The administrator's login (the password is asked on the terminal)
    #[arg(long, value_name = "LOGIN")]
    pub(super) login: String,
    #[command(flatten)]
    pub(super) yes: YesArgs,
    #[command(flatten)]
    pub(super) instance: InstanceArgs,
}

const FIELDS: [Column; 3] = [
    column("instance", "instance", Show::Name),
    column("origin", "origin", Show::Plain),
    column("sha256_fingerprint", "sha256_fingerprint", Show::Id),
];

pub(super) fn run(args: SetArgs, ctx: &mut ExecutionContext) -> Result<(), CliError> {
    if ctx.modes().dry_run {
        let plan = json!({"action": "admin_tls_set", "cert": args.cert, "key": args.key});
        let summary = format!(
            "would replace the HTTPS pair with {} and {}",
            args.cert, args.key
        );
        return Ok(ctx.render_dry_run(&summary, &plan)?);
    }
    require_confirmation(ctx, "Replace this instance's HTTPS certificate and key")?;
    let target = selection::select(args.instance.name.as_deref(), None)?;
    let replaced = org_host::setup_flow::admin_tls_set(
        target.instance(),
        std::path::Path::new(&args.cert),
        std::path::Path::new(&args.key),
        &args.login,
    )?;
    let json = view::json(&replaced)?;
    view::detail(ctx, &json, &json, "sha256_fingerprint", &FIELDS)?;
    ctx.success("Replaced the HTTPS certificate and key; the host serves them now.");
    Ok(())
}
