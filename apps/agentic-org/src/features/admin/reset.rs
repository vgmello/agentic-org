//! `admin reset`: a new one-time administrator link, shown on the terminal only.

use clap::Args;
use org_cli_shell::context::ExecutionContext;
use org_cli_shell::error::ShellError;
use org_cli_shell::prompt::confirm;
use org_cli_shell::terminal::sanitize;
use org_host_setup::{AdminLink, AdminResetLink};
use serde_json::json;

use crate::args::{InstanceArgs, YesArgs};
use crate::error::CliError;
use crate::selection;
use crate::view;

pub(super) const EXAMPLES: &str = "Examples:\n  agentic-org admin reset\n  sudo agentic-org admin reset --name lab\n  docker exec -it -u 0 <container> agentic-org admin reset --user <service user>";

pub(super) const LONG_ABOUT: &str = "Issue a new one-time administrator registration link after local confirmation
Use when: issuing a fresh one-time administrator registration or password-reset link after local OS confirmation (cancels any previous unused link).

On a Mac: asks for Touch ID or your login password, then prints a new administrator registration link. On Linux: run it as root (sudo, or --user <service user> where there is no sudo, as in a container). For an instance set up with XDG_DATA_HOME, XDG_CACHE_HOME or XDG_STATE_HOME set, give root the same values: sudo env XDG_DATA_HOME=<dir> XDG_CACHE_HOME=<dir> agentic-org admin reset --user <service user>. The previous link stops working.";

#[derive(Args, Debug)]
pub(crate) struct ResetArgs {
    /// The service user whose instance root acts on (Linux, run as root)
    #[arg(long, value_name = "USER")]
    pub(super) user: Option<String>,
    #[command(flatten)]
    pub(super) yes: YesArgs,
    #[command(flatten)]
    pub(super) instance: InstanceArgs,
}

/// The link is a one-time secret: it is shown only on a terminal (the host refuses a pipe
/// itself, before it asks anything). A person at that terminal is asked first, since the
/// previous unused link stops working; where no one can be asked, the host's own rule (a
/// terminal, or root on Linux) decides.
pub(super) fn run(args: ResetArgs, ctx: &mut ExecutionContext) -> Result<(), CliError> {
    let terminal = ctx.is_tty_stdout();
    let target = selection::select(args.instance.name.as_deref(), None)?;
    // The host's own refusals (SSH on a Mac, not a terminal, not root on Linux) come first:
    // a person is never asked and then refused.
    org_host_setup::admin_reset_precheck(target.instance(), args.user.as_deref(), terminal)?;
    if ctx.modes().dry_run {
        let plan = json!({"action": "admin_reset", "user": args.user});
        return Ok(ctx.render_dry_run("would issue a new one-time administrator link", &plan)?);
    }
    confirm_reset(ctx)?;
    let link = org_host_setup::admin_reset(target.instance(), args.user.as_deref(), terminal)?;
    if ctx.modes().json {
        return Ok(ctx.write_json(&link)?);
    }
    show(ctx, &link)
}

/// Any earlier unused link stops working, so a person at the terminal is asked (no by
/// default), and a run that opted out of questions (`--json`, `-q`, `--no-input`) needs
/// `--yes`. Where no one can be asked only because there is no input terminal, the host's own
/// confirmation (Touch ID or the login password on a Mac, root on Linux) is what guards it.
fn confirm_reset(ctx: &mut ExecutionContext) -> Result<(), CliError> {
    let modes = ctx.modes();
    if modes.yes {
        return Ok(());
    }
    if ctx.can_prompt() {
        let question = "Issue a new administrator link? Any earlier unused link stops working";
        if !confirm(ctx, question, false)? {
            return Err(ShellError::Cancelled.into());
        }
        return Ok(());
    }
    if modes.json || modes.quiet || modes.no_input {
        return Err(ShellError::ConfirmationRequired {
            action: "Issue a new administrator link".to_owned(),
        }
        .into());
    }
    Ok(())
}

fn show(ctx: &mut ExecutionContext, link: &AdminResetLink) -> Result<(), CliError> {
    let (opening, url, expires) = match &link.link {
        AdminLink::Registration { url, expires_at_ms } => (
            "Open this link in a browser to create the administrator account.".to_owned(),
            url.expose(),
            *expires_at_ms,
        ),
        AdminLink::ResetPassword {
            login,
            url,
            expires_at_ms,
        } => (
            format!(
                "Open this link in a browser to set a new password for {}.",
                sanitize(login)
            ),
            url.expose(),
            *expires_at_ms,
        ),
        AdminLink::AlreadyIssued => {
            if ctx.modes().quiet {
                return Ok(());
            }
            return Ok(ctx.out(&format!(
                "For a new link, run: {}\n",
                sanitize(&link.reset_command)
            ))?);
        }
    };
    if ctx.modes().quiet {
        return Ok(ctx.out(&format!("{}\n", sanitize(url)))?);
    }
    // SIGN-33: root's reset names the Linux user whose instance it acted on, first.
    // Every value is escaped on its own: a line break inside one cannot start a line.
    let whose = match &link.service_user {
        Some(user) => format!(
            "Instance {} of Linux user {}.\n",
            sanitize(&link.instance),
            sanitize(user)
        ),
        None => String::new(),
    };
    let text = format!(
        "{whose}\n{opening}\nIt works once and expires in 24 hours (at {}):\n  {}\nIf it expires or is lost, run: {}\n",
        view::utc_time(expires),
        sanitize(url),
        sanitize(&link.reset_command)
    );
    Ok(ctx.out(&org_cli_shell::terminal::sanitize_lines(&text))?)
}

#[cfg(test)]
mod tests {
    use org_cli_shell::context::testing::{captured, tty};
    use org_cli_shell::context::Modes;
    use org_host_setup::{AdminLink, AdminResetLink, OneTimeLinkUrl};

    use super::show;

    /// Values the host relays: a name a service user chose, and the command to run again.
    fn hostile(link: AdminLink) -> AdminResetLink {
        AdminResetLink {
            instance: "lab\u{1b}[2J".into(),
            service_user: Some("svc\u{202e}gnp".into()),
            link,
            reset_command: "agentic-org admin reset\n\u{1b}]0;pwned\u{7}".into(),
        }
    }

    fn registration() -> AdminLink {
        AdminLink::Registration {
            url: OneTimeLinkUrl::new("http://127.0.0.1:7420/register#abc".into()).unwrap(),
            expires_at_ms: 0,
        }
    }

    /// B-06: every value `admin reset` prints is escaped, in every form: no escape sequence,
    /// bidirectional override or line break from a value reaches the terminal.
    #[test]
    fn every_relayed_value_is_escaped_in_every_form() {
        for (quiet, link) in [
            (false, registration()),
            (true, registration()),
            (false, AdminLink::AlreadyIssued),
        ] {
            let modes = Modes {
                quiet,
                ..Modes::default()
            };
            let (mut ctx, streams) = captured(modes, tty(Some(80)), &[]);
            show(&mut ctx, &hostile(link)).unwrap();
            let shown = streams.stdout.text();
            for raw in ['\u{1b}', '\u{202e}', '\u{7}'] {
                assert!(!shown.contains(raw), "{raw:?} is raw in {shown:?}");
            }
            assert!(
                !shown.contains("\n\\u{1b}]0"),
                "a value began a line: {shown:?}"
            );
            if !quiet {
                assert!(
                    shown.contains("admin reset\\u{a}\\u{1b}]0;pwned"),
                    "{shown:?}"
                );
            }
        }
    }
}
