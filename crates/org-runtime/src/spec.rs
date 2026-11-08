//! What the office asks for: [`SessionSpec`] (stored, hashed, replayed on every start and
//! resume) and [`LaunchContext`] (per run, never stored by the adapter, never hashed),
//! including whether the run resumes a conversation.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    AgentName, Capabilities, CommandPrefix, EnvName, McpServerName, PathPattern, PlainId, RuleTool,
    RuntimeId, SessionId, SpecPath, Token, ToolName,
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

/// Writes `value` as compact JSON with every object's keys sorted.
///
/// Sorting here rather than relying on `serde_json::Map`'s order keeps the form the same
/// even if some crate in the build turns on serde_json's `preserve_order` feature.
fn write_canonical(value: &Value, out: &mut Vec<u8>) {
    match value {
        Value::Object(map) => {
            let mut entries: Vec<(&String, &Value)> = map.iter().collect();
            entries.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
            out.push(b'{');
            for (i, (key, item)) in entries.into_iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                write_scalar(&Value::String(key.clone()), out);
                out.push(b':');
                write_canonical(item, out);
            }
            out.push(b'}');
        }
        Value::Array(items) => {
            out.push(b'[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                write_canonical(item, out);
            }
            out.push(b']');
        }
        scalar => write_scalar(scalar, out),
    }
}

fn write_scalar(value: &Value, out: &mut Vec<u8>) {
    serde_json::to_writer(out, value).expect("writing a JSON scalar to a Vec can't fail");
}

/// 64-bit FNV-1a. Pure, tiny, and stable forever, which is all a change detector needs.
fn fnv1a_64(bytes: &[u8]) -> u64 {
    const OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    bytes.iter().fold(OFFSET_BASIS, |hash, &byte| {
        (hash ^ u64::from(byte)).wrapping_mul(PRIME)
    })
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
    /// [`SessionSpec::instructions`]. Free text that may reach argv or a file: never a secret.
    pub prompt: String,
}

/// How the runtime asks before acting. Each mode maps to every runtime's own setting
/// (Claude `--permission-mode`, Codex `approvalPolicy`, Antigravity's mode).
///
/// **There is no bypass variant**, and a stored or sent bypass value (e.g.
/// `"bypassPermissions"`) fails to decode, so no code path can ask a runtime to skip its
/// permission checks. Like every boundary enum it's `#[non_exhaustive]`: an adapter that
/// meets a mode it doesn't know refuses the spec ([`crate::PlanError::Unsupported`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum PermissionMode {
    /// Ask before anything the policy doesn't allow (Claude's "manual" / "default").
    Ask,
    /// Accept file edits inside the writable roots without asking; ask for the rest. An
    /// implicit allow, so adapters apply it only through
    /// [`SessionSpec::effective_permission_mode`], which turns it into `Ask` when a deny was
    /// dropped.
    AcceptEdits,
    /// Plan only: read and propose, never change anything.
    Plan,
}

/// The rules the runtime applies itself, before anything reaches the office's approval
/// bridge. The office answers bridged approvals from the same policy.
///
/// **A deny beats a matching allow, whatever the order of the rules**: the lists aren't
/// ordered rule chains, and an adapter whose runtime is first-match-wins writes every deny
/// before every allow.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PermissionPolicy {
    /// The policy's version in the office (every change, `allow_always` included, bumps it).
    pub version: u64,
    /// What the runtime may do without asking. Grants: an adapter drops one its runtime can't
    /// express exactly (never widens it), and drops them all when it drops the deny rules
    /// (see [`crate::Capabilities`]).
    pub allow: Vec<Rule>,
    /// What the runtime must never do. Deny wins over allow. A droppable restriction, per
    /// [`crate::Capabilities::enforces_deny_rules`], never narrowed.
    pub deny: Vec<Rule>,
}

/// One permission rule: a tool plus what it applies to.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    /// The tool, in the runtime's naming (e.g. `Bash`, `Edit`), or `*` for any tool.
    pub tool: RuleTool,
    /// What the rule covers.
    pub scope: RuleScope,
}

/// What a [`Rule`] covers. Written by the office, so an unknown scope fails to decode.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
#[non_exhaustive]
pub enum RuleScope {
    /// Every use of the tool.
    Any,
    /// Uses on paths matching this pattern.
    Path(PathPattern),
    /// Shell commands that start with this prefix, for shell tools.
    CommandPrefix(CommandPrefix),
    /// Tools of this MCP server.
    McpServer(McpServerName),
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
    /// Unix sockets the agent may connect to: its own office socket, for `org-agent`.
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
    /// The session's own `TMPDIR`. The office sets the variable; **every adapter adds it as a
    /// writable root** (it isn't in [`SandboxSpec::writable_roots`], being per run). A **real
    /// path**, like `runtime_dir` (the user's temp folder is under a symlink on macOS).
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
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(path: &str) -> SpecPath {
        SpecPath::try_from(path).unwrap()
    }

    fn pat(pattern: &str) -> PathPattern {
        PathPattern::try_from(pattern).unwrap()
    }

    fn tok(token: &str) -> Token {
        Token::try_from(token).unwrap()
    }

    fn mcp(name: &str) -> McpServerName {
        McpServerName::try_from(name).unwrap()
    }

    fn sample_spec() -> SessionSpec {
        let mut mcp_servers = BTreeMap::new();
        mcp_servers.insert(
            mcp("office"),
            McpServer {
                command: p("/office/bin/org-mcp"),
                args: vec!["--stdio".to_owned()],
            },
        );
        SessionSpec {
            runtime: RuntimeId::try_from("claude").unwrap(),
            kind: SessionKind::Task {
                task_id: PlainId::try_from("t1").unwrap(),
            },
            agent: AgentIdentity {
                name: AgentName::try_from("ac-dev-ada").unwrap(),
                description: "Staff engineer".to_owned(),
                prompt: "You are ada.".to_owned(),
            },
            model: Some(tok("opus")),
            effort: None,
            workspace: p("/work/t-1"),
            read_only_dirs: vec![p("/homes/ada")],
            instructions: "Do the task.\nThen stop.".to_owned(),
            permission_mode: PermissionMode::AcceptEdits,
            policy: PermissionPolicy {
                version: 3,
                allow: vec![Rule {
                    tool: RuleTool::try_from("Bash").unwrap(),
                    scope: RuleScope::CommandPrefix(CommandPrefix::try_from("git ").unwrap()),
                }],
                deny: vec![Rule {
                    tool: RuleTool::try_from("*").unwrap(),
                    scope: RuleScope::Path(pat("/office/**")),
                }],
            },
            sandbox: SandboxSpec {
                enabled: true,
                writable_roots: vec![p("/work/t-1")],
                deny_write: vec![pat("/work/t-1/.git/hooks/**")],
                deny_read: vec![pat("/office/offices/*/**")],
                allowed_unix_sockets: vec![p("/office/run/ada.sock")],
            },
            mcp_servers,
            tools: Some(vec![
                ToolName::try_from("Bash").unwrap(),
                ToolName::try_from("Edit").unwrap(),
            ]),
        }
    }

    #[test]
    fn canonical_json_is_compact_with_sorted_keys() {
        let spec = sample_spec();
        let json = String::from_utf8(spec.canonical_json()).unwrap();
        // The exact form is part of the contract (it feeds every stored hash).
        let expected = concat!(
            r#"{"agent":{"description":"Staff engineer","name":"ac-dev-ada","prompt":"You are ada."},"#,
            r#""effort":null,"instructions":"Do the task.\nThen stop.","kind":{"task_id":"t1","type":"task"},"#,
            r#""mcp_servers":{"office":{"args":["--stdio"],"command":"/office/bin/org-mcp"}},"#,
            r#""model":"opus","permission_mode":"accept_edits","#,
            r#""policy":{"allow":[{"scope":{"type":"command_prefix","value":"git "},"tool":"Bash"}],"#,
            r#""deny":[{"scope":{"type":"path","value":"/office/**"},"tool":"*"}],"version":3},"#,
            r#""read_only_dirs":["/homes/ada"],"runtime":"claude","#,
            r#""sandbox":{"allowed_unix_sockets":["/office/run/ada.sock"],"#,
            r#""deny_read":["/office/offices/*/**"],"deny_write":["/work/t-1/.git/hooks/**"],"#,
            r#""enabled":true,"writable_roots":["/work/t-1"]},"tools":["Bash","Edit"],"#,
            r#""workspace":"/work/t-1"}"#
        );
        assert_eq!(json, expected);
        // The canonical form is itself valid JSON that decodes to the same spec.
        let back: SessionSpec = serde_json::from_str(&json).unwrap();
        assert_eq!(back, spec);
    }

    #[test]
    fn spec_hash_is_pinned() {
        // Pins the algorithm and the canonical form: if this changes, every stored hash
        // changes, which is a contract change.
        let spec = sample_spec();
        // The sample's hash, computed independently (FNV-1a 64 over the golden canonical JSON
        // in `canonical_json_is_compact_with_sorted_keys`).
        assert_eq!(spec.spec_hash(), "4ca16808942e59dc");
        assert_eq!(
            spec.spec_hash(),
            format!("{:016x}", fnv1a_64(&spec.canonical_json()))
        );
        assert_eq!(spec.spec_hash().len(), 16);
        assert!(spec
            .spec_hash()
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)));
        // FNV-1a 64 reference vectors.
        assert_eq!(fnv1a_64(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a_64(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(fnv1a_64(b"foobar"), 0x8594_4171_f739_67e8);
    }

    #[test]
    fn map_insertion_order_does_not_matter() {
        let mut a = sample_spec();
        let mut b = sample_spec();
        let server = McpServer {
            command: p("/x"),
            args: vec![],
        };
        a.mcp_servers.clear();
        b.mcp_servers.clear();
        for name in ["zeta", "alpha", "mid"] {
            a.mcp_servers.insert(mcp(name), server.clone());
        }
        for name in ["mid", "zeta", "alpha"] {
            b.mcp_servers.insert(mcp(name), server.clone());
        }
        assert_eq!(a.canonical_json(), b.canonical_json());
        assert_eq!(a.spec_hash(), b.spec_hash());
    }

    #[test]
    fn canonical_form_does_not_depend_on_the_input_key_order() {
        // A spec decoded from JSON with shuffled keys and whitespace hashes the same.
        let spec = sample_spec();
        let mut value = serde_json::to_value(&spec).unwrap();
        let shuffled: serde_json::Map<String, Value> = value
            .as_object_mut()
            .unwrap()
            .iter()
            .rev()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let pretty = serde_json::to_string_pretty(&Value::Object(shuffled)).unwrap();
        let back: SessionSpec = serde_json::from_str(&pretty).unwrap();
        assert_eq!(back.spec_hash(), spec.spec_hash());
        assert_eq!(back.canonical_json(), spec.canonical_json());
    }

    #[test]
    fn spec_hash_is_stable_across_clone_and_serde_round_trip() {
        let spec = sample_spec();
        let hash = spec.spec_hash();
        assert_eq!(spec.clone().spec_hash(), hash);
        let stored = serde_json::to_string(&spec).unwrap();
        let back: SessionSpec = serde_json::from_str(&stored).unwrap();
        assert_eq!(back, spec);
        assert_eq!(back.spec_hash(), hash);
        // Twice, to be sure it isn't something the first decode normalized.
        let again: SessionSpec =
            serde_json::from_str(&serde_json::to_string(&back).unwrap()).unwrap();
        assert_eq!(again.spec_hash(), hash);
    }

    #[test]
    fn spec_hash_changes_when_any_field_changes() {
        let base = sample_spec();
        let base_hash = base.spec_hash();
        // One mutation per field, listed by an exhaustive destructuring so a new field
        // can't be added without a case here.
        let SessionSpec {
            runtime: _,
            kind: _,
            agent: _,
            model: _,
            effort: _,
            workspace: _,
            read_only_dirs: _,
            instructions: _,
            permission_mode: _,
            policy: _,
            sandbox: _,
            mcp_servers: _,
            tools: _,
        } = &base;
        type Mutation = (&'static str, fn(&mut SessionSpec));
        let mutations: Vec<Mutation> = vec![
            ("runtime", |s| {
                s.runtime = RuntimeId::try_from("codex").unwrap()
            }),
            ("kind", |s| s.kind = SessionKind::Home {}),
            ("kind.task_id", |s| {
                s.kind = SessionKind::Task {
                    task_id: PlainId::try_from("t2").unwrap(),
                }
            }),
            ("kind one-off", |s| {
                s.kind = SessionKind::OneOff {
                    run_id: PlainId::try_from("t1").unwrap(),
                }
            }),
            ("agent.name", |s| {
                s.agent.name = AgentName::try_from("ac-dev-ada2").unwrap()
            }),
            ("agent.description", |s| s.agent.description.push('x')),
            ("agent.prompt", |s| s.agent.prompt.push('x')),
            ("model", |s| s.model = None),
            ("effort", |s| s.effort = Some(tok("high"))),
            ("workspace", |s| s.workspace = p("/work/t-2")),
            ("read_only_dirs", |s| s.read_only_dirs.clear()),
            ("instructions", |s| s.instructions.push('!')),
            ("permission_mode", |s| {
                s.permission_mode = PermissionMode::Plan
            }),
            ("policy.version", |s| s.policy.version += 1),
            ("policy.allow", |s| s.policy.allow.clear()),
            ("policy.deny", |s| s.policy.deny.clear()),
            ("policy rule scope", |s| {
                s.policy.allow[0].scope = RuleScope::Path(pat("/git"))
            }),
            ("sandbox.enabled", |s| s.sandbox.enabled = false),
            ("sandbox.writable_roots", |s| {
                s.sandbox.writable_roots.push(p("/tmp/x"))
            }),
            ("sandbox.deny_write", |s| s.sandbox.deny_write.clear()),
            ("sandbox.deny_read", |s| s.sandbox.deny_read.clear()),
            ("sandbox.allowed_unix_sockets", |s| {
                s.sandbox.allowed_unix_sockets.clear()
            }),
            ("mcp_servers", |s| s.mcp_servers.clear()),
            ("mcp_servers.args", |s| {
                s.mcp_servers.get_mut(&mcp("office")).unwrap().args.clear()
            }),
            ("tools", |s| s.tools = None),
            ("tools list", |s| {
                s.tools = Some(vec![ToolName::try_from("Bash").unwrap()])
            }),
            ("list order", |s| {
                s.read_only_dirs.push(p("/a"));
                s.read_only_dirs.reverse();
            }),
        ];
        let mut seen = std::collections::BTreeSet::new();
        seen.insert(base_hash.clone());
        for (name, mutate) in mutations {
            let mut spec = base.clone();
            mutate(&mut spec);
            assert_ne!(spec, base, "{name}: the mutation changed nothing");
            let hash = spec.spec_hash();
            assert_ne!(hash, base_hash, "{name}: hash didn't change");
            assert!(seen.insert(hash), "{name}: hash collides with another case");
        }
    }

    fn caps(deny_rules: bool, deny_write: bool, deny_read: bool, tools: bool) -> Capabilities {
        Capabilities {
            steer: false,
            interrupt_in_band: true,
            idle_interrupt_ends_turn: false,
            approval_bridge: true,
            deny_message_reaches_model: true,
            runtime_cancels_approvals: false,
            unsolicited_turns: false,
            session_id_known_up_front: true,
            resume_ignores_cwd: false,
            resume_applies_instructions: true,
            reads_workspace_instruction_files: false,
            tools_detach_from_group: false,
            take_over: true,
            enforces_writable_roots: true,
            enforces_deny_write: deny_write,
            enforces_deny_read: deny_read,
            enforces_unix_socket_allowlist: true,
            enforces_tool_allowlist: tools,
            enforces_deny_rules: deny_rules,
            usage_scope: crate::UsageScope::TurnDelta,
            cost: crate::CostBasis::None,
            tested_versions: crate::VersionRange {
                min: "1".into(),
                max: "2".into(),
            },
        }
    }

    /// The sample with every deny-like restriction empty or off.
    fn no_denies() -> SessionSpec {
        let mut spec = sample_spec();
        spec.policy.deny.clear();
        spec.sandbox.deny_write.clear();
        spec.sandbox.deny_read.clear();
        spec.tools = None;
        spec
    }

    #[test]
    fn deny_wins_over_allow_for_each_restriction_on_its_own() {
        let all = caps(true, true, true, true);
        let empty = no_denies();
        assert!(empty.keeps_allow_rules(&all));
        // Nothing to drop: no capability matters, sandbox on or off.
        assert!(empty.keeps_allow_rules(&caps(false, false, false, false)));
        let mut off = empty.clone();
        off.sandbox.enabled = false;
        assert!(off.keeps_allow_rules(&caps(false, false, false, false)));

        type Add = fn(&mut SessionSpec);
        // (what, how to make only it non-empty, the caps that drop only it, whether turning
        // the sandbox off drops it)
        let cases: [(&str, Add, Capabilities, bool); 4] = [
            (
                "deny rules",
                |s| s.policy.deny = sample_spec().policy.deny,
                caps(false, true, true, true),
                false,
            ),
            (
                "deny_write",
                |s| s.sandbox.deny_write = sample_spec().sandbox.deny_write,
                caps(true, false, true, true),
                true,
            ),
            (
                "deny_read",
                |s| s.sandbox.deny_read = sample_spec().sandbox.deny_read,
                caps(true, true, false, true),
                true,
            ),
            (
                "tools",
                |s| s.tools = Some(vec![]),
                caps(true, true, true, false),
                false,
            ),
        ];
        for (what, add, drops_it, sandbox_off_drops_it) in cases.clone() {
            let mut spec = no_denies();
            add(&mut spec);
            assert!(spec.keeps_allow_rules(&all), "{what}: enforced");
            assert!(!spec.keeps_allow_rules(&drops_it), "{what}: dropped");
            assert_eq!(
                spec.effective_permission_mode(&drops_it),
                PermissionMode::Ask,
                "{what}: accept_edits must narrow"
            );
            assert_eq!(
                spec.effective_permission_mode(&all),
                PermissionMode::AcceptEdits
            );
            // Dropping only the others changes nothing.
            for (other, _, other_caps, _) in &cases {
                if *other != what {
                    assert!(spec.keeps_allow_rules(other_caps), "{what} vs {other}");
                }
            }
            let mut off = spec.clone();
            off.sandbox.enabled = false;
            assert_eq!(
                !off.keeps_allow_rules(&all),
                sandbox_off_drops_it,
                "{what}: sandbox off"
            );
        }
    }

    #[test]
    fn effective_permission_mode_only_narrows() {
        let dropped = caps(false, true, true, true);
        let mut spec = sample_spec();
        for mode in [
            PermissionMode::Ask,
            PermissionMode::AcceptEdits,
            PermissionMode::Plan,
        ] {
            spec.permission_mode = mode;
            let expected = if mode == PermissionMode::AcceptEdits {
                PermissionMode::Ask
            } else {
                mode
            };
            assert_eq!(spec.effective_permission_mode(&dropped), expected);
            assert_eq!(
                spec.effective_permission_mode(&caps(true, true, true, true)),
                mode
            );
        }
    }

    #[test]
    fn permission_mode_has_no_bypass() {
        // Compile-level: an exhaustive match inside the crate. Adding a variant (a bypass
        // one included) breaks this test, so it can't happen without a deliberate edit here.
        fn runtime_checks_permissions(mode: PermissionMode) -> bool {
            match mode {
                PermissionMode::Ask | PermissionMode::AcceptEdits | PermissionMode::Plan => true,
            }
        }
        for mode in [
            PermissionMode::Ask,
            PermissionMode::AcceptEdits,
            PermissionMode::Plan,
        ] {
            assert!(runtime_checks_permissions(mode));
            let json = serde_json::to_string(&mode).unwrap();
            assert_eq!(serde_json::from_str::<PermissionMode>(&json).unwrap(), mode);
        }
        assert_eq!(
            serde_json::to_string(&PermissionMode::AcceptEdits).unwrap(),
            r#""accept_edits""#
        );
        // Serde refuses every spelling of a bypass request, and anything unknown.
        for bad in [
            r#""bypassPermissions""#,
            r#""bypass_permissions""#,
            r#""bypass""#,
            r#""dangerously_skip_permissions""#,
            r#""danger-full-access""#,
            r#""unknown""#,
            r#""""#,
        ] {
            assert!(
                serde_json::from_str::<PermissionMode>(bad).is_err(),
                "{bad} decoded"
            );
        }
        // And a stored spec carrying one doesn't decode at all.
        let mut value = serde_json::to_value(sample_spec()).unwrap();
        value["permission_mode"] = Value::String("bypassPermissions".to_owned());
        assert!(serde_json::from_value::<SessionSpec>(value).is_err());
    }

    #[test]
    fn unknown_spec_fields_fail_to_decode_at_every_level() {
        let base = serde_json::to_value(sample_spec()).unwrap();
        assert!(serde_json::from_value::<SessionSpec>(base.clone()).is_ok());
        for pointer in [
            "",
            "/kind",
            "/agent",
            "/policy",
            "/policy/allow/0",
            "/policy/allow/0/scope",
            "/sandbox",
            "/mcp_servers/office",
        ] {
            let mut value = base.clone();
            value
                .pointer_mut(pointer)
                .unwrap()
                .as_object_mut()
                .unwrap()
                .insert("future_restriction".to_owned(), Value::Bool(true));
            assert!(
                serde_json::from_value::<SessionSpec>(value).is_err(),
                "an unknown field at {pointer:?} decoded"
            );
        }
        // Beside a fieldless variant's tag too.
        let mut value = base.clone();
        value["kind"] = serde_json::json!({"type": "home", "future_restriction": true});
        assert!(serde_json::from_value::<SessionSpec>(value).is_err());
        let mut value = base;
        value["kind"] = serde_json::json!({"type": "home"});
        assert_eq!(
            serde_json::from_value::<SessionSpec>(value).unwrap().kind,
            SessionKind::Home {}
        );
    }

    #[test]
    fn v1_option_keys_must_be_present() {
        let base = serde_json::to_value(sample_spec()).unwrap();
        for key in ["tools", "model", "effort"] {
            let mut value = base.clone();
            value.as_object_mut().unwrap().remove(key);
            let err = serde_json::from_value::<SessionSpec>(value).unwrap_err();
            assert!(err.to_string().contains("missing field"), "{key}: {err}");
            // Present as null is fine: that's None.
            let mut value = base.clone();
            value[key] = Value::Null;
            assert!(
                serde_json::from_value::<SessionSpec>(value).is_ok(),
                "{key}"
            );
        }
    }

    #[test]
    fn office_written_enums_fail_closed_on_unknown_values() {
        assert!(serde_json::from_str::<SessionKind>(r#"{"type":"shared"}"#).is_err());
        assert!(serde_json::from_str::<RuleScope>(r#"{"type":"regex","value":".*"}"#).is_err());
    }

    #[test]
    fn spec_parts_round_trip() {
        for kind in [
            SessionKind::Home {},
            SessionKind::Task {
                task_id: PlainId::try_from("t").unwrap(),
            },
            SessionKind::OneOff {
                run_id: PlainId::try_from("r").unwrap(),
            },
        ] {
            let json = serde_json::to_string(&kind).unwrap();
            assert_eq!(serde_json::from_str::<SessionKind>(&json).unwrap(), kind);
        }
        assert_eq!(
            serde_json::to_string(&SessionKind::Home {}).unwrap(),
            r#"{"type":"home"}"#
        );
        for scope in [
            RuleScope::Any,
            RuleScope::Path(pat("/a/**")),
            RuleScope::CommandPrefix(CommandPrefix::try_from("git ").unwrap()),
            RuleScope::McpServer(mcp("office")),
        ] {
            let json = serde_json::to_string(&scope).unwrap();
            assert_eq!(serde_json::from_str::<RuleScope>(&json).unwrap(), scope);
        }
        assert_eq!(
            serde_json::to_string(&RuleScope::Any).unwrap(),
            r#"{"type":"any"}"#
        );
    }

    #[test]
    fn a_stored_spec_with_a_bad_value_does_not_decode() {
        let base = serde_json::to_value(sample_spec()).unwrap();
        for (pointer, bad) in [
            ("/workspace", "work/t-1"),
            ("/read_only_dirs/0", "/homes/ada/../x"),
            ("/sandbox/writable_roots/0", "/work//t-1"),
            ("/sandbox/allowed_unix_sockets/0", "/office/run/"),
            ("/mcp_servers/office/command", "org-mcp"),
            ("/sandbox/deny_write/0", "~/.claude/**"),
            ("/sandbox/deny_read/0", "/office/offices/*/*.db"),
            ("/workspace", "/"),
            ("/sandbox/writable_roots/0", "/"),
            ("/policy/deny/0/scope/value", "/office/**/x*"),
            ("/policy/allow/0/scope/value", "git (status)"),
            ("/agent/name", "Ada"),
            ("/model", "opus --dangerously-skip-permissions"),
            ("/model", "-x"),
            ("/tools/0", "Bash(git *)"),
            ("/kind/task_id", "t-1"),
            ("/policy/allow/0/tool", "Bash) Read(/etc"),
            ("/policy/deny/0/scope/value", "/work/[draft]"),
            ("/sandbox/deny_read/0", "/work/a?"),
        ] {
            let mut value = base.clone();
            *value.pointer_mut(pointer).unwrap() = Value::from(bad);
            assert!(
                serde_json::from_value::<SessionSpec>(value).is_err(),
                "{pointer} = {bad:?} decoded"
            );
        }
        // Map keys are checked too.
        let mut value = base;
        let server = value["mcp_servers"]["office"].clone();
        value["mcp_servers"]["my server"] = server;
        assert!(serde_json::from_value::<SessionSpec>(value).is_err());
    }
}
