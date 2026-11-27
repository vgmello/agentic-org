//! The shape of one feature slice as the registry composes it: its `org-command` rows, the
//! `Use when:` guidance of its resources and commands, and the dispatch that runs its requests.
//! A slice declares one [`Domain`]; the registry lists them, so adding a command touches its
//! slice and nothing else.

use org_command::{Answer, Command, CommandTable, Refusal, Request};

use crate::runtime::transport::OfficeTransport;

/// `(resource, Use when)` guidance of one resource.
pub(crate) type ResourceCue = (&'static str, &'static str);

/// `(resource, verb, Use when)` guidance of one command.
pub(crate) type CommandCue = (&'static str, &'static str, &'static str);

/// Runs `request` over the transport when the slice owns its command; `None` for any other.
/// The table is the live one (for a command that answers with the catalogue).
pub(crate) type Dispatch =
    fn(&mut dyn OfficeTransport, &Request, &CommandTable) -> Option<Result<Answer, Refusal>>;

/// One feature slice.
pub(crate) struct Domain {
    /// The slice's rows, one slice of rows per resource file.
    pub(crate) commands: &'static [&'static [Command]],
    /// Guidance for each resource the slice owns.
    pub(crate) resources: &'static [ResourceCue],
    /// Guidance for each command the slice owns, one slice per resource file.
    pub(crate) cues: &'static [&'static [CommandCue]],
    pub(crate) dispatch: Dispatch,
}

impl Domain {
    /// Whether this slice owns `command` in the shared [`org_command_catalogue`] table.
    pub(crate) fn owns(&self, command: &Command) -> bool {
        self.commands
            .iter()
            .copied()
            .flatten()
            .any(|row| row.resource == command.resource && row.verb == command.verb)
    }
}

/// The guidance of `resource` in `domains`.
pub(crate) fn resource_cue(domains: &[Domain], resource: &str) -> Option<&'static str> {
    domains
        .iter()
        .flat_map(|domain| domain.resources.iter())
        .find(|(name, _)| *name == resource)
        .map(|(_, cue)| *cue)
}

/// The guidance of the command `resource verb` in `domains`.
pub(crate) fn command_cue(domains: &[Domain], resource: &str, verb: &str) -> Option<&'static str> {
    domains
        .iter()
        .flat_map(|domain| domain.cues.iter().copied().flatten())
        .find(|(r, v, _)| *r == resource && *v == verb)
        .map(|(_, _, cue)| *cue)
}
