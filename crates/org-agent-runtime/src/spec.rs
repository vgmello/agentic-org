//! What the office asks for: [`SessionSpec`] (stored, hashed, replayed on every start and
//! resume) and [`LaunchContext`] (per run, never stored by the adapter, never hashed),
//! including whether the run resumes a conversation.

mod canonical;
mod policy;

#[cfg(test)]
mod tests;

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::{
    AgentName, Capabilities, EnvName, McpServerName, PathPattern, PlainId, RuntimeId, SessionId,
    SpecPath, Token, ToolName,
};

use canonical::{fnv1a_64, write_canonical};
pub use policy::{
    PermissionMode, PermissionPolicy, Rule, RuleScope, ToolCapability, CAPABILITY_PREFIX,
};

/// Everything the office asks a runtime for, for one session.
///
/// The office stores it with the session record and **replays the whole spec on every start
/// and resume**, because no runtime restores its settings on resume (design decision 5).
/// It's deterministic: only ordered collections (`Vec`, `BTreeMap`), and no per-run value.
/// The session token, the session mark, the session's `TMPDIR` and the conversation to resume
/// ([`LaunchContext::resume`]) belong in [`LaunchContext`], so every run of the same session,
/// first start and resumes alike, hashes the same.
///
/// Lists keep their order in [`canonical_json`](Self::canonical_json), so the office must
/// build them in a stable order (sorted, or in a fixed order of its own).
///
/// **Unknown fields fail to decode**, here and in every part of the spec: a field this build
/// doesn't know may be a restriction (a new deny list, say), and ignoring it would silently
/// weaken isolation. An older build refuses a newer spec rather than run it partly.
///
/// Every name, token, path and pattern in it is a validated type, so nothing in the spec can
/// break out of the argv, config file or rule it's written into. Free text
/// ([`instructions`](Self::instructions), the agent's description and prompt, MCP server
/// arguments) may reach argv or files as is, so **the office keeps secrets out of it**.
///
/// **Every v1 field is required**, `Option`s included: their keys must be present in a stored
/// spec (as `null` for `None`), so a spec that lost a key fails to decode instead of reading
/// as "no restriction" (a missing `tools` must not mean every tool).
///
/// **Evolution:** a field added later must be optional, with `#[serde(default,
/// skip_serializing_if = ...)]`, so that existing specs keep decoding and keep their
/// [`spec_hash`](Self::spec_hash), and older builds still read specs that don't use it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionSpec {
    /// Which runtime runs the session, e.g. `claude`, `codex`, `agy`.
    pub runtime: RuntimeId,
    /// A home session, a task session or a one-off.
    pub kind: SessionKind,
    /// The agent as the runtime should know it.
    pub agent: AgentIdentity,
    /// The model, in the runtime's own naming. `None` keeps the runtime's default. A v1
    /// field, so its key must be present in a stored spec (as `null` for `None`).
    #[serde(deserialize_with = "present")]
    pub model: Option<Token>,
    /// The reasoning effort, in the runtime's own naming. `None` keeps the runtime's default.
    /// A v1 field, so its key must be present (as `null` for `None`).
    #[serde(deserialize_with = "present")]
    pub effort: Option<Token>,
    /// The session's working directory: the agent's home for a home session, the task's
    /// worktree for a task session. Every resume uses it too (Codex and Antigravity filter
    /// conversations by cwd). Never `/`: a stored spec with it doesn't decode, and an adapter
    /// refuses one built in code ([`crate::PlanError::InvalidSpec`]).
    #[serde(deserialize_with = "not_root")]
    pub workspace: SpecPath,
    /// Directories the agent may read but never write: the agent's home, for a task session.
    /// A grant of read access (see [`crate::Capabilities`]); while the writable roots are
    /// enforced they're never writable, because only [`SandboxSpec::writable_roots`] are.
    pub read_only_dirs: Vec<SpecPath>,
    /// The office's composed instructions (identity, memory, project files, wake guidance).
    /// The adapter passes them the runtime's own way (a file for Claude,
    /// `developerInstructions` for Codex, the agent file for Antigravity). Free text that
    /// reaches a file or a protocol message: never a secret.
    pub instructions: String,
    /// The project repository's instruction files (`doc-27` §1, Part 7), as the office composed
    /// them. Held apart from [`instructions`](Self::instructions) so that
    /// [`standing_prompt`](Self::standing_prompt) keeps them last by structure, whatever text
    /// the organization's parts carry. Free text that reaches a file or a protocol message:
    /// never a secret. Added after v1, so optional: absent when empty, which keeps every
    /// existing spec's [`canonical_json`](Self::canonical_json) and hash.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub repository_instructions: String,
    /// How the runtime asks before acting. There is no bypass mode.
    pub permission_mode: PermissionMode,
    /// The runtime's own first line of defence, set from the office's policy.
    pub policy: PermissionPolicy,
    /// The runtime's sandbox.
    pub sandbox: SandboxSpec,
    /// MCP servers the office provides, by name. Only these load: the adapter turns off every
    /// server from the user's own config, or refuses the spec if it can't. Often empty.
    pub mcp_servers: BTreeMap<McpServerName, McpServer>,
    /// The tools the agent may use, or `None` for the runtime's default set. `Some(vec![])`
    /// means no tools at all. A restriction: an adapter whose runtime can't limit tools drops
    /// it, and says so with [`crate::Capabilities::enforces_tool_allowlist`]. A v1 field, so
    /// its key must be present (as `null` for `None`): a stored spec that lost it must not
    /// decode as "every tool".
    #[serde(deserialize_with = "present")]
    pub tools: Option<Vec<ToolName>>,
}

impl SessionSpec {
    /// The whole standing system prompt in static-to-volatile order (`doc-27` §1): the office's
    /// [`instructions`](Self::instructions) first, so the release-static start of the
    /// instructions is byte 0 of what the runtime caches, then the agent's own
    /// [`prompt`](AgentIdentity::prompt), then the project repository's
    /// [`repository_instructions`](Self::repository_instructions) last. Each part is trimmed and
    /// empty parts are left out; parts are joined by a blank line. Empty when every part is.
    pub fn standing_prompt(&self) -> String {
        [
            self.instructions.as_str(),
            self.agent.prompt.as_str(),
            self.repository_instructions.as_str(),
        ]
        .iter()
        .map(|part| part.trim())
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
    }

    /// The spec's canonical JSON: the input to [`spec_hash`](Self::spec_hash), and a stable
    /// form to diff two specs by.
    ///
    /// It's the spec's serde form, written with **every object's keys sorted** by their UTF-8
    /// bytes (struct fields and map keys alike, so neither declaration order nor insertion
    /// order matters), **no whitespace**, strings escaped as `serde_json` escapes them, and
    /// integers in decimal. Arrays keep their order. `None` is written as `null`.
    ///
    /// It's for hashing and comparing; store the spec with its ordinary serde form.
    ///
    /// It's total: every path, pattern, name and token is a validated UTF-8 string by type,
    /// and every other field is a string, an integer, a bool or a string-keyed map, all of
    /// which JSON can always hold.
    pub fn canonical_json(&self) -> Vec<u8> {
        let value = serde_json::to_value(self)
            .expect("unreachable: every SessionSpec field has a JSON representation");
        let mut out = Vec::new();
        write_canonical(&value, &mut out);
        out
    }

    /// A change detector over the whole spec: 16 lowercase hex digits of the 64-bit FNV-1a
    /// hash of [`canonical_json`](Self::canonical_json).
    ///
    /// Equal specs always have equal hashes, across clones, serde round trips and builds.
    /// Resuming doesn't change it (the conversation to resume is [`LaunchContext::resume`],
    /// not part of the spec). A different hash says only that **some** field changed: every
    /// field is covered, [`PermissionPolicy::version`] included (so every `allow_always`
    /// changes it). To learn *what* changed (e.g. whether a resume must tell the agent its
    /// instructions changed), compare the stored spec's fields, such as
    /// [`instructions`](Self::instructions), with the new one's.
    ///
    /// **It isn't a security hash:** it's easy to forge a collision, so never use it to
    /// authenticate or deduplicate untrusted input.
    pub fn spec_hash(&self) -> String {
        format!("{:016x}", fnv1a_64(&self.canonical_json()))
    }

    /// Whether an adapter with these capabilities may apply the policy's `allow` rules.
    ///
    /// **Deny wins over allow:** when the adapter drops any restriction that denies something
    /// (the policy's non-empty `deny` rules, the sandbox's non-empty `deny_write` or
    /// `deny_read` because its `enforces_*` field is false or the sandbox is off, or a `tools`
    /// allowlist it can't enforce), an allow rule could let the runtime act without asking on
    /// something that restriction covered. So then it drops **every** allow rule, and
    /// everything reaches the approval bridge, where the office applies its full policy. The
    /// same goes for the implicit allow in [`PermissionMode::AcceptEdits`]: see
    /// [`effective_permission_mode`](Self::effective_permission_mode). Every adapter asks these
    /// rather than deciding it itself.
    pub fn keeps_allow_rules(&self, caps: &Capabilities) -> bool {
        let sandbox = &self.sandbox;
        let drops = |list_is_empty: bool, enforced: bool| !list_is_empty && !enforced;
        let drops_tools = self.tools.is_some() && !caps.enforces_tool_allowlist;
        let drops_deny_rules = drops(self.policy.deny.is_empty(), caps.enforces_deny_rules);
        let drops_deny_write = drops(
            sandbox.deny_write.is_empty(),
            sandbox.enabled && caps.enforces_deny_write,
        );
        let drops_deny_read = drops(
            sandbox.deny_read.is_empty(),
            sandbox.enabled && caps.enforces_deny_read,
        );
        !(drops_tools || drops_deny_rules || drops_deny_write || drops_deny_read)
    }

    /// The permission mode an adapter with these capabilities applies: the spec's, except
    /// that [`PermissionMode::AcceptEdits`] becomes [`PermissionMode::Ask`] when
    /// [`keeps_allow_rules`](Self::keeps_allow_rules) is false. Accepting edits is an
    /// implicit allow, so like the explicit ones it gives way when a deny it could get around
    /// was dropped (e.g. edits under a dropped `deny_write` of `.git/hooks/**`). That only
    /// narrows: the session asks more often.
    pub fn effective_permission_mode(&self, caps: &Capabilities) -> PermissionMode {
        match self.permission_mode {
            PermissionMode::AcceptEdits if !self.keeps_allow_rules(caps) => PermissionMode::Ask,
            mode => mode,
        }
    }
}

/// Decodes a path that may not be `/` (a workspace).
fn not_root<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<SpecPath, D::Error> {
    let path = SpecPath::deserialize(deserializer)?;
    if path.is_root() {
        return Err(serde::de::Error::custom("`/` can't be a workspace"));
    }
    Ok(path)
}

/// Decodes a list of paths none of which may be `/` (writable roots).
fn none_is_root<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<SpecPath>, D::Error> {
    let paths = Vec::<SpecPath>::deserialize(deserializer)?;
    if paths.iter().any(SpecPath::is_root) {
        return Err(serde::de::Error::custom("`/` can't be a writable root"));
    }
    Ok(paths)
}

/// Decodes an `Option` whose key must be present (`null` for `None`). Serde's derive treats a
/// missing `Option` key as `None`; a field with its own `deserialize_with` and no `default`
/// makes the key required instead.
fn present<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}

/// Which kind of session this is. Written by the office, so an unknown kind fails to decode.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum SessionKind {
    /// The agent's home session, for triggers that aren't about a task.
    ///
    /// A struct variant with no fields, not a unit variant, so that an unknown field next to
    /// its tag fails to decode like everywhere else in the spec (serde ignores extra fields
    /// beside a unit variant's tag). It's still `{"type":"home"}` on the wire.
    Home {},
    /// The session for one task, in that task's worktree.
    Task {
        /// The task, as the office names it.
        task_id: PlainId,
    },
    /// A one-off run. It has no Member and can't be taken over.
    OneOff {
        /// The run, as the office names it.
        run_id: PlainId,
    },
}

/// The agent as the runtime should know it. Claude defines it inline (`--agents`) and selects
/// it (`--agent`); Antigravity reads it from a generated agent file; Codex has no agent
/// concept and uses only the instructions.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentIdentity {
    /// The runtime-safe name the runtime selects the agent by.
    pub name: AgentName,
    /// A short description, where the runtime shows one. Free text that may reach argv or a
    /// file: never a secret.
    pub description: String,
    /// The agent definition's own one-line prompt. The real instructions go in
    /// [`SessionSpec::instructions`]; adapters place this after them
    /// ([`SessionSpec::standing_prompt`]). Free text that may reach argv or a file: never a
    /// secret.
    pub prompt: String,
}

/// The runtime's sandbox. On by default (design decision 6).
///
/// Writable roots and sockets are literal paths that may not exist yet (git creates some of
/// them on the first commit). The deny lists are [`PathPattern`]s.
///
/// Each list is a restriction an adapter may be unable to **enforce** on its runtime: then
/// it drops that list and says so up front in [`crate::Capabilities`]
/// (`enforces_writable_roots`, `enforces_deny_write`, `enforces_deny_read`,
/// `enforces_unix_socket_allowlist`), so the office decides before it plans (see
/// [`crate::AgentRuntime::plan_session`]).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SandboxSpec {
    /// Whether the runtime's sandbox is on. `true` with a runtime that has no sandbox is
    /// refused; `false` drops the lists below (see [`crate::Capabilities`]).
    pub enabled: bool,
    /// Where the agent may write: the workspace, and the parts of `work.git` a commit needs.
    /// Not the session's `TMPDIR`: it's per run, so it isn't in the spec, and **every adapter
    /// always adds [`LaunchContext::tmpdir`] as a writable root** itself. Never `/` (that
    /// would make everything writable): a stored spec with it doesn't decode, and an adapter
    /// refuses one built in code ([`crate::PlanError::InvalidSpec`]).
    #[serde(deserialize_with = "none_is_root")]
    pub writable_roots: Vec<SpecPath>,
    /// Never writable, even inside a writable root (the fixed denies, git admin files).
    pub deny_write: Vec<PathPattern>,
    /// Never readable: every office's whole folder (`<office home>/offices/*/**`, not just
    /// its databases), the office's run folder, runtime stores.
    pub deny_read: Vec<PathPattern>,
    /// Unix sockets the agent may connect to: its own office socket, for `agent-os`.
    pub allowed_unix_sockets: Vec<SpecPath>,
}

/// An MCP server the office provides, run over stdio.
///
/// It inherits the session's environment and has no environment of its own, because the spec
/// is stored and hashed and must never hold a secret.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct McpServer {
    /// The server's program, by absolute path.
    pub command: SpecPath,
    /// Its arguments. Free text that reaches argv or a config file: never a secret.
    pub args: Vec<String>,
}

/// Facts about one run (one process's life) that aren't part of the spec: they change from
/// run to run, so they're never hashed, and the adapter uses them only to build the plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchContext {
    /// The runtime's binary, by the real path recorded at preflight. Never looked up on `PATH`.
    pub runtime_path: PathBuf,
    /// The runtime's version, as recorded at preflight, for gating flags.
    pub runtime_version: Option<String>,
    /// Where the plan's [`crate::FileLocation::RuntimeDir`] files go
    /// (`<office dir>/agents/<id>/runtime`, or a one-off's own folder). The agent can read
    /// it, never write it. A **real path**: the office resolves every symlink in it first
    /// (e.g. `/var` to `/private/var` on macOS), so it matches what sandboxes see.
    pub runtime_dir: SpecPath,
    /// The session's own `TMPDIR`. Never `/`: an adapter refuses one
    /// ([`crate::PlanError::InvalidSpec`]). The office sets the variable; **every adapter adds
    /// it as a writable root** (it isn't in [`SandboxSpec::writable_roots`], being per run).
    /// A **real path**, like `runtime_dir` (the user's temp folder is under a symlink on
    /// macOS).
    pub tmpdir: SpecPath,
    /// The **names** of the variables the office puts in the child's environment
    /// (`ORG_SESSION_TOKEN`, `ORG_AGENT`, ...), for runtimes that must pass them on to their
    /// tools by name (Codex `shell_environment_policy.include_only`). The values never reach
    /// the adapter.
    pub office_env_names: Vec<EnvName>,
    /// An id the office minted for a new conversation, for runtimes that take one up front
    /// (Claude `--session-id`). The port can't make one: it has no randomness. Unused when
    /// [`resume`](Self::resume) is set.
    pub new_session_id: Option<SessionId>,
    /// The runtime's session id of the conversation to resume, or `None` to start a new one.
    ///
    /// It's per run rather than part of the [`SessionSpec`], so resuming a session replays
    /// the same spec with the same [`SessionSpec::spec_hash`] as its first start.
    pub resume: Option<SessionId>,
    /// Optional non-secret real path to the agent's private vendor config directory
    /// (`CLAUDE_CONFIG_DIR` for Claude, `CODEX_HOME` for Codex; ADR-0015 decision 10).
    /// Never `/` (an adapter refuses one with [`crate::PlanError::InvalidSpec`]) and never a
    /// credential value.
    pub config_dir: Option<SpecPath>,
}
