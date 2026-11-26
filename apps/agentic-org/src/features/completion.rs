//! `agentic-org completion <shell>`: the shell's completion script. Answered before any
//! configuration is read, so a broken `agentic-org.toml` never blocks it.

use clap::Args;
use clap_complete::{Generator, Shell};
use org_cli_shell::context::ExecutionContext;

use crate::error::CliError;

const EXAMPLES: &str = "Examples:
  agentic-org completion zsh > ~/.zfunc/_agentic-org
  agentic-org completion bash > ~/.local/share/bash-completion/completions/agentic-org
  agentic-org completion fish --dynamic > ~/.config/fish/completions/agentic-org.fish

With --dynamic the script asks `agentic-org` itself on every <TAB> (the same as
`COMPLETE=<shell> agentic-org`): it completes instance names, organizations, offices and
projects from the cache the list commands fill, without contacting a host.";

#[derive(Args, Debug)]
#[command(after_help = EXAMPLES)]
pub(crate) struct CompletionArgs {
    /// The shell to complete for
    #[arg(value_enum)]
    shell: Shell,
    /// Print the dynamic registration (candidates computed on every <TAB>) instead of a
    /// static script
    #[arg(long)]
    dynamic: bool,
}

/// Writes the script for `command` (the whole grammar) to standard output.
pub(crate) fn run(
    args: CompletionArgs,
    mut command: clap::Command,
    ctx: &mut ExecutionContext,
) -> Result<(), CliError> {
    let mut buf = Vec::new();
    if args.dynamic {
        let shells = clap_complete::env::Shells::builtins();
        let shell = shells
            .completer(&args.shell.to_string())
            .ok_or_else(|| CliError::usage(format!("no dynamic completion for {}", args.shell)))?;
        shell
            .write_registration(
                "COMPLETE",
                "agentic-org",
                "agentic-org",
                "agentic-org",
                &mut buf,
            )
            .map_err(|e| CliError::failed("cli::io", e.to_string()))?;
    } else {
        command.set_bin_name("agentic-org");
        command.build();
        args.shell.generate(&command, &mut buf);
    }
    Ok(ctx.out(&String::from_utf8_lossy(&buf))?)
}
