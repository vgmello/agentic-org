//! Permission presets, default-on sandbox, and escalation policy (`ADR-0032` §3, `ENROLL-7`, `FC-189`).

use std::fmt;

use org_agent_runtime::{
    ApprovalKind, ApprovalRequest, CommandPrefix, Escalation, PathPattern, PermissionMode,
    PermissionPolicy, Rule, RuleScope, RuleTool, SandboxSpec, SpecPath, ToolCapability, ToolName,
};
use org_office::organization_family::records::PermissionPreset;
use org_proto::sessions::{GrantRuleView, SessionGrantsResult};

/// Effective configuration derived from a [`PermissionPreset`] and sandbox toggle.
#[derive(Debug, Clone)]
pub struct PresetConfig {
    pub preset: PermissionPreset,
    pub permission_mode: PermissionMode,
    pub policy: PermissionPolicy,
    pub sandbox: SandboxSpec,
    pub allowed_tools: Option<Vec<ToolName>>,
}

/// Errors building a permission/sandbox configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PresetError {
    BypassRefused(&'static str),
    InvalidPath(String),
}

impl fmt::Display for PresetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BypassRefused(mode) => {
                write!(
                    f,
                    "bypass permission mode `{mode}` is unconditionally refused"
                )
            }
            Self::InvalidPath(detail) => write!(f, "invalid sandbox path: {detail}"),
        }
    }
}

impl std::error::Error for PresetError {}

/// Parses a wire/API preset name, unconditionally refusing any bypass/unrestricted mode (`ENROLL-7`).
pub fn parse_preset(name: &str) -> Result<PermissionPreset, PresetError> {
    let trimmed = name.trim();
    if matches!(
        trimmed,
        "bypass"
            | "bypassPermissions"
            | "bypass_permissions"
            | "dangerously_skip_permissions"
            | "unrestricted"
    ) {
        return Err(PresetError::BypassRefused("bypassPermissions"));
    }
    PermissionPreset::parse(trimmed).ok_or(PresetError::BypassRefused("unknown permission preset"))
}

/// Builds the runtime's [`PermissionMode`], [`PermissionPolicy`], [`SandboxSpec`], and tool list
/// from `preset`, `sandbox_enabled` (default-on), workspace, office socket, and denied roots.
pub fn build_session_policy(
    preset: PermissionPreset,
    sandbox_enabled: bool,
    workspace: &SpecPath,
    office_socket: Option<&SpecPath>,
    denied_office_globs: &[PathPattern],
    policy_version: u64,
) -> Result<PresetConfig, PresetError> {
    // No trailing whitespace so Claude Code emits `Bash(agent-os *)` instead of dropping the rule.
    let agent_os_prefix =
        CommandPrefix::try_from("agent-os").expect("static agent-os command prefix is valid");
    let (permission_mode, allow_rules, allowed_tools) = match preset {
        PermissionPreset::SafeAsk => (PermissionMode::Ask, Vec::new(), None),
        PermissionPreset::ReadOnly => (
            PermissionMode::Plan,
            vec![
                cap_rule(ToolCapability::Read, RuleScope::Any),
                cap_rule(ToolCapability::Search, RuleScope::Any),
                cap_rule(
                    ToolCapability::Shell,
                    RuleScope::CommandPrefix(agent_os_prefix),
                ),
            ],
            Some(vec![
                ToolCapability::Read.to_tool_name(),
                ToolCapability::Search.to_tool_name(),
                ToolCapability::Shell.to_tool_name(),
            ]),
        ),
        PermissionPreset::WorkspaceEdits => (
            PermissionMode::AcceptEdits,
            vec![
                cap_rule(ToolCapability::Read, RuleScope::Any),
                cap_rule(ToolCapability::Search, RuleScope::Any),
                cap_rule(ToolCapability::Edit, RuleScope::Any),
                cap_rule(
                    ToolCapability::Shell,
                    RuleScope::CommandPrefix(agent_os_prefix),
                ),
            ],
            None,
        ),
        PermissionPreset::AutonomousSandboxed => (
            PermissionMode::AcceptEdits,
            vec![
                cap_rule(ToolCapability::Read, RuleScope::Any),
                cap_rule(ToolCapability::Search, RuleScope::Any),
                cap_rule(ToolCapability::Edit, RuleScope::Any),
                cap_rule(ToolCapability::Shell, RuleScope::Any),
            ],
            None,
        ),
    };

    let mut deny_rules = Vec::new();
    let mut deny_read = Vec::new();
    let mut deny_write = Vec::new();
    for office_glob in denied_office_globs {
        deny_rules.push(Rule {
            tool: RuleTool::try_from("*").expect("wildcard rule tool"),
            scope: RuleScope::Path(office_glob.clone()),
        });
        deny_read.push(office_glob.clone());
        deny_write.push(office_glob.clone());
    }
    let hooks_glob = format!("{}/.git/hooks/**", workspace.as_str());
    if let Ok(git_hooks) = PathPattern::try_from(hooks_glob.as_str()) {
        deny_rules.push(Rule {
            tool: RuleTool::try_from("*").expect("wildcard rule tool"),
            scope: RuleScope::Path(git_hooks.clone()),
        });
        deny_write.push(git_hooks);
    }

    let writable_roots = match preset {
        PermissionPreset::ReadOnly => Vec::new(),
        _ => vec![workspace.clone()],
    };
    let allowed_unix_sockets = office_socket.into_iter().cloned().collect();

    Ok(PresetConfig {
        preset,
        permission_mode,
        policy: PermissionPolicy {
            version: policy_version.max(1),
            allow: allow_rules,
            deny: deny_rules,
        },
        sandbox: SandboxSpec {
            enabled: sandbox_enabled,
            writable_roots,
            deny_write,
            deny_read,
            allowed_unix_sockets,
        },
        allowed_tools,
    })
}

/// Extends a [`PresetConfig`] with agent-owned task worktree writable roots and bare-cache
/// write-only denies (`doc-35` / `FC-103` / `TASK-126`) without denying read access to shared
/// Git metadata (`HEAD`, `config`, `packed-refs`, `refs/remotes`).
pub fn apply_worktree_sandbox_rules(
    cfg: &mut PresetConfig,
    extra_writable_roots: &[SpecPath],
    extra_write_denies: &[PathPattern],
) {
    if cfg.preset != PermissionPreset::ReadOnly {
        for root in extra_writable_roots {
            if !cfg.sandbox.writable_roots.contains(root) {
                cfg.sandbox.writable_roots.push(root.clone());
            }
        }
    }
    let edit_tool = RuleTool::try_from(ToolCapability::Edit.as_rule_token())
        .expect("static edit capability token is valid");
    for pat in extra_write_denies {
        if !cfg.sandbox.deny_write.contains(pat) {
            cfg.sandbox.deny_write.push(pat.clone());
        }
        let deny_rule = Rule {
            tool: edit_tool.clone(),
            scope: RuleScope::Path(pat.clone()),
        };
        if !cfg.policy.deny.contains(&deny_rule) {
            cfg.policy.deny.push(deny_rule);
        }
    }
}

/// How the office answers an incoming [`ApprovalRequest`] from a runtime session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EscalationDisposition {
    /// Allowed automatically by the session's active policy (only when `Escalation::None`).
    AllowByPolicy,
    /// Denied automatically without bothering the owner (fixed deny or `SandboxBypass`).
    AutoDeny { reason: String },
    /// Escalated to the organization owner with an explicit warning (never answered from policy).
    EscalateToOwner { warning: String },
}

/// Evaluates an [`ApprovalRequest`] against the session's policy and ADR-0032 escalation rules.
/// - `Escalation::SandboxBypass` is ALWAYS auto-denied (fixed denies win; no override in v1).
/// - `Escalation::PermissionChange` and `Escalation::Unknown` ALWAYS go to the owner.
/// - `Escalation::None` checks deny rules first (deny beats allow, including scoped Path /
///   CommandPrefix / McpServer rules; fails closed on opaque shell invocations when path denies exist).
pub fn decide_escalation(
    request: &ApprovalRequest,
    policy: &PermissionPolicy,
    preset: PermissionPreset,
) -> EscalationDisposition {
    decide_escalation_with_tools(request, policy, preset, None)
}

/// Evaluates an [`ApprovalRequest`] against the session's policy, optional `allowed_tools`
/// allowlist, and ADR-0032 escalation rules.
///
/// Hard refusals (`SandboxBypass`, policy `deny` rules, and `allowed_tools` allowlist
/// exclusions) are ALWAYS applied before routing escalated requests (`PermissionChange` /
/// `Unknown`) to the owner so an escalated request can never bypass fixed denies or a
/// `ReadOnly` tool allowlist.
pub fn decide_escalation_with_tools(
    request: &ApprovalRequest,
    policy: &PermissionPolicy,
    preset: PermissionPreset,
    allowed_tools: Option<&[ToolName]>,
) -> EscalationDisposition {
    decide_escalation_with_roots(request, policy, preset, allowed_tools, None)
}

/// Evaluates an [`ApprovalRequest`] against the session's policy, optional `allowed_tools`
/// allowlist, optional sandbox `writable_roots` (for file-editing tools running outside OS shell
/// confinement such as Claude's built-in `Write`/`Edit` tools, per `doc-29`), and ADR-0032
/// escalation rules.
pub fn decide_escalation_with_roots(
    request: &ApprovalRequest,
    policy: &PermissionPolicy,
    preset: PermissionPreset,
    allowed_tools: Option<&[ToolName]>,
    writable_roots: Option<&[SpecPath]>,
) -> EscalationDisposition {
    let escalation = request.effective_escalation();
    if escalation == Escalation::SandboxBypass {
        return EscalationDisposition::AutoDeny {
            reason: "Sandbox bypass requests are refused by office security policy.".to_owned(),
        };
    }

    let mut strings = Vec::new();
    collect_json_strings(&request.input, &mut strings);
    if let Some(blocked_path) = &request.blocked_path {
        strings.push(blocked_path.clone());
    }
    // Include extracted concrete file targets (including map-shaped `changes` object keys and
    // rename destinations) so every target path is checked against fixed path denies.
    let (concrete_targets, has_opaque_targets) = extract_concrete_file_targets(request);
    for target in &concrete_targets {
        if !strings.contains(target) {
            strings.push(target.clone());
        }
    }
    // Build a separate deny-check vector augmented with canonicalized/normalized target paths
    // so `/./`, `//`, OS path aliases (`/var` -> `/private/var`), and in-root symlinks cannot
    // evade fixed path denies while keeping `matches_any_allow` strictly on raw `strings`.
    let deny_strings = build_deny_strings(&strings, &concrete_targets);
    if matches_any_deny(&policy.deny, request, &deny_strings) {
        return EscalationDisposition::AutoDeny {
            reason: format!(
                "Tool `{}` is denied by office security policy.",
                request.tool
            ),
        };
    }
    if let Some(allowlist) = allowed_tools {
        if !request_in_allowed_tools(request, allowlist) {
            return EscalationDisposition::AutoDeny {
                reason: format!(
                    "Tool `{}` is outside this session's allowed tools.",
                    request.tool
                ),
            };
        }
    }
    if let Some(roots) = writable_roots {
        if is_edit_request(request)
            && (has_opaque_targets || !all_targets_within_writable_roots(&concrete_targets, roots))
        {
            return EscalationDisposition::AutoDeny {
                reason: format!(
                    "Write target for tool `{}` is outside this session's writable roots.",
                    request.tool
                ),
            };
        }
    }

    match escalation {
        Escalation::PermissionChange => EscalationDisposition::EscalateToOwner {
            warning: format!(
                "Agent requested a permission mode change for tool `{}`; owner decision required.",
                request.tool
            ),
        },
        Escalation::None => evaluate_allow_policy(request, policy, preset, &strings),
        _ => EscalationDisposition::EscalateToOwner {
            warning: format!(
                "Agent raised an unrecognized escalation for tool `{}`; owner decision required.",
                request.tool
            ),
        },
    }
}

fn is_edit_request(request: &ApprovalRequest) -> bool {
    request.kind == ApprovalKind::FileChange || request.capability == Some(ToolCapability::Edit)
}

fn push_unique_path(out: &mut Vec<String>, path: Option<std::path::PathBuf>) {
    let Some(s) = path.as_deref().and_then(std::path::Path::to_str) else {
        return;
    };
    if !out.iter().any(|existing| existing == s) {
        out.push(s.to_owned());
    }
}

fn build_deny_strings(raw_strings: &[String], concrete_targets: &[String]) -> Vec<String> {
    let mut deny_strings = raw_strings.to_vec();
    for target in concrete_targets {
        let p = std::path::Path::new(target);
        push_unique_path(&mut deny_strings, resolve_path_for_root_check(p));
        push_unique_path(&mut deny_strings, resolve_path_for_deny(p));
        if p.is_absolute() {
            push_unique_path(&mut deny_strings, p.canonicalize().ok());
            push_unique_path(&mut deny_strings, lexical_normalize_abs(p));
        }
    }
    deny_strings
}

fn push_deny_components(
    path: &std::path::Path,
    queue: &mut std::collections::VecDeque<std::ffi::OsString>,
    prepend: bool,
) {
    let mut parts: Vec<std::ffi::OsString> = Vec::new();
    for comp in path.components() {
        match comp {
            std::path::Component::RootDir => parts.push(std::path::MAIN_SEPARATOR_STR.into()),
            std::path::Component::Prefix(pref) => parts.push(pref.as_os_str().to_os_string()),
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => parts.push("..".into()),
            std::path::Component::Normal(seg) => parts.push(seg.to_os_string()),
        }
    }
    if prepend {
        for part in parts.into_iter().rev() {
            queue.push_front(part);
        }
    } else {
        queue.extend(parts);
    }
}

/// Bounded component-by-component path resolver used strictly for deny matching (`matches_any_deny`).
/// Follows both live and dangling symlinks via `read_link` and collapses `..` while canonicalizing
/// existing prefixes so unsandboxed (`writable_roots = None`) sessions still block writes through
/// dangling symlinks or symlink+`..` traversals into denied targets.
fn resolve_path_for_deny(path: &std::path::Path) -> Option<std::path::PathBuf> {
    const MAX_SYMLINK_HOPS: usize = 40;
    if !path.is_absolute() {
        return None;
    }
    let mut queue = std::collections::VecDeque::new();
    push_deny_components(path, &mut queue, false);
    let mut cur = std::path::PathBuf::new();
    let mut hops = 0usize;
    while let Some(seg) = queue.pop_front() {
        if seg == std::path::MAIN_SEPARATOR_STR {
            cur = std::path::PathBuf::from(std::path::MAIN_SEPARATOR_STR);
            continue;
        }
        if seg == ".." {
            if cur.parent().is_some() {
                cur.pop();
            }
            continue;
        }
        let next = cur.join(&seg);
        if let Ok(target) = std::fs::read_link(&next) {
            hops += 1;
            if hops > MAX_SYMLINK_HOPS {
                return None;
            }
            if target.is_absolute() {
                cur = std::path::PathBuf::from(std::path::MAIN_SEPARATOR_STR);
            }
            push_deny_components(&target, &mut queue, true);
            continue;
        }
        cur = next.canonicalize().unwrap_or(next);
    }
    cur.is_absolute().then_some(cur)
}

fn lexical_normalize_abs(path: &std::path::Path) -> Option<std::path::PathBuf> {
    if !path.is_absolute() {
        return None;
    }
    let mut out = std::path::PathBuf::new();
    for comp in path.components() {
        match comp {
            std::path::Component::RootDir => out.push(std::path::MAIN_SEPARATOR_STR),
            std::path::Component::Prefix(pref) => out.push(pref.as_os_str()),
            std::path::Component::Normal(seg) => out.push(seg),
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if out.parent().is_some() {
                    out.pop();
                }
            }
        }
    }
    Some(out)
}

fn all_targets_within_writable_roots(targets: &[String], roots: &[SpecPath]) -> bool {
    if targets.is_empty() || roots.is_empty() {
        return false;
    }
    let canon_roots: Vec<std::path::PathBuf> = roots
        .iter()
        .filter_map(|r| resolve_path_for_root_check(std::path::Path::new(r.as_str())))
        // Never accept filesystem root `/` as a writable root.
        .filter(|canon| canon.parent().is_some())
        .collect();
    if canon_roots.is_empty() {
        return false;
    }
    targets
        .iter()
        .all(|t| target_within_canonical_roots(t, &canon_roots))
}

fn target_within_canonical_roots(raw: &str, canon_roots: &[std::path::PathBuf]) -> bool {
    let Some(resolved) = resolve_path_for_root_check(std::path::Path::new(raw)) else {
        return false;
    };
    canon_roots.iter().any(|root| resolved.starts_with(root))
}

fn normalize_strict_abs_path(path: &std::path::Path) -> Option<std::path::PathBuf> {
    let raw = path.to_str()?;
    if raw.is_empty() || raw.contains('\0') {
        return None;
    }
    if !path.is_absolute()
        || path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return None;
    }
    // Rebuilding from `components()` strips trailing `/`, `/.`, and duplicate separators so
    // POSIX `lstat` (`symlink_metadata`) never dereferences a trailing-slash dangling symlink.
    let clean: std::path::PathBuf = path.components().collect();
    clean.is_absolute().then_some(clean)
}

fn resolve_path_for_root_check(path: &std::path::Path) -> Option<std::path::PathBuf> {
    let clean = normalize_strict_abs_path(path)?;
    if let Ok(canon) = clean.canonicalize() {
        return Some(canon);
    }
    let mut tail = Vec::new();
    let mut cur: &std::path::Path = &clean;
    while let Some(parent) = cur.parent() {
        // Fail closed on any symlink (leaf or intermediate) or non-NotFound metadata error
        // (ELOOP, EACCES, ENOTDIR, InvalidInput) so dangling symlinks cannot escape roots.
        match std::fs::symlink_metadata(cur) {
            Ok(meta) if meta.file_type().is_symlink() => return None,
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return None,
        }
        if let Some(name) = cur.file_name() {
            tail.push(name.to_os_string());
        }
        if let Ok(mut base) = parent.canonicalize() {
            for seg in tail.into_iter().rev() {
                base.push(seg);
            }
            return Some(base);
        }
        cur = parent;
    }
    None
}

fn evaluate_allow_policy(
    request: &ApprovalRequest,
    policy: &PermissionPolicy,
    preset: PermissionPreset,
    strings: &[String],
) -> EscalationDisposition {
    if request.capability == Some(ToolCapability::Unknown) {
        return EscalationDisposition::EscalateToOwner {
            warning: format!(
                "Tool `{}` reported an unrecognized capability class; owner decision required.",
                request.tool
            ),
        };
    }
    if has_path_deny(&policy.deny, request) {
        if is_shell_request(request)
            && (strings.is_empty() || extract_shell_command(&request.input).is_none())
        {
            return EscalationDisposition::EscalateToOwner {
                warning: format!(
                    "Shell tool `{}` with opaque input cannot be proven outside denied office paths.",
                    request.tool
                ),
            };
        }
        if is_file_target_request(request) && !has_concrete_file_target(request) {
            return EscalationDisposition::EscalateToOwner {
                warning: format!(
                    "File tool `{}` with opaque input cannot be proven outside denied office paths.",
                    request.tool
                ),
            };
        }
    }
    if has_command_prefix_deny(&policy.deny, request)
        && is_shell_request(request)
        && !shell_command_is_unambiguously_decoded(&request.input)
    {
        return EscalationDisposition::EscalateToOwner {
            warning: format!(
                "Shell tool `{}` with compound or undecodable command cannot be proven outside denied command prefixes.",
                request.tool
            ),
        };
    }
    if preset != PermissionPreset::SafeAsk && matches_any_allow(&policy.allow, request, strings) {
        return EscalationDisposition::AllowByPolicy;
    }
    EscalationDisposition::EscalateToOwner {
        warning: format!(
            "Approval required for tool `{}` under `{}` preset.",
            request.tool,
            preset.as_str()
        ),
    }
}

/// Projects a [`PresetConfig`] into the wire [`SessionGrantsResult`] served by `agent-os grant list`.
pub fn build_grants_summary(
    employee_id: &str,
    config: &PresetConfig,
    read_only_dirs: &[SpecPath],
) -> SessionGrantsResult {
    let mut rules = Vec::new();
    for rule in &config.policy.deny {
        rules.push(rule_to_view("deny", rule));
    }
    for rule in &config.policy.allow {
        rules.push(rule_to_view("allow", rule));
    }
    SessionGrantsResult {
        employee_id: employee_id.to_owned(),
        permission_preset: config.preset.as_str().to_owned(),
        permission_mode: mode_name(config.permission_mode).to_owned(),
        sandbox_enabled: config.sandbox.enabled,
        writable_roots: config
            .sandbox
            .writable_roots
            .iter()
            .map(|p| p.as_str().to_owned())
            .collect(),
        read_only_dirs: read_only_dirs
            .iter()
            .map(|p| p.as_str().to_owned())
            .collect(),
        denied_write_patterns: config
            .sandbox
            .deny_write
            .iter()
            .map(|p| p.as_str().to_owned())
            .collect(),
        denied_read_patterns: config
            .sandbox
            .deny_read
            .iter()
            .map(|p| p.as_str().to_owned())
            .collect(),
        allowed_tools: config
            .allowed_tools
            .as_ref()
            .map(|list| list.iter().map(|t| t.as_str().to_owned()).collect()),
        rules,
    }
}

fn cap_rule(cap: ToolCapability, scope: RuleScope) -> Rule {
    Rule {
        tool: cap.to_rule_tool(),
        scope,
    }
}

fn collect_json_strings(value: &serde_json::Value, out: &mut Vec<String>) {
    match value {
        serde_json::Value::String(s) => out.push(s.clone()),
        serde_json::Value::Array(items) => {
            for item in items {
                collect_json_strings(item, out);
            }
        }
        serde_json::Value::Object(map) => {
            for v in map.values() {
                collect_json_strings(v, out);
            }
        }
        _ => {}
    }
}

fn is_shell_request(request: &ApprovalRequest) -> bool {
    request.capability == Some(ToolCapability::Shell) || request.kind == ApprovalKind::Command
}

fn is_file_target_request(request: &ApprovalRequest) -> bool {
    request.kind == ApprovalKind::FileChange
        || matches!(
            request.capability,
            Some(ToolCapability::Edit | ToolCapability::Read)
        )
}

const MOVE_DEST_KEYS: &[&str] = &[
    "move_path",
    "movePath",
    "new_path",
    "newPath",
    "destination",
];

fn extract_optional_path_keys(
    map: &serde_json::Map<String, serde_json::Value>,
    keys: &[&str],
    targets: &mut Vec<String>,
    opaque: &mut bool,
) {
    for key in keys {
        if let Some(val) = map.get(*key).filter(|v| !v.is_null()) {
            match val.as_str().filter(|s| !s.trim().is_empty()) {
                Some(s) => targets.push(s.to_owned()),
                None => *opaque = true,
            }
        }
    }
}

fn extract_change_dest_paths(
    map: &serde_json::Map<String, serde_json::Value>,
    targets: &mut Vec<String>,
    opaque: &mut bool,
) {
    extract_optional_path_keys(map, MOVE_DEST_KEYS, targets, opaque);
    if let Some(kind_map) = map.get("kind").and_then(serde_json::Value::as_object) {
        extract_optional_path_keys(kind_map, MOVE_DEST_KEYS, targets, opaque);
    }
}

const CHANGE_ITEM_PATH_KEYS: &[&str] = &[
    "path",
    "file_path",
    "filePath",
    "notebook_path",
    "notebookPath",
    "TargetFile",
    "AbsolutePath",
    "target_file",
];

fn extract_change_array_item(
    item: &serde_json::Value,
    targets: &mut Vec<String>,
    opaque: &mut bool,
) {
    let Some(map) = item.as_object() else {
        *opaque = true;
        return;
    };
    let before = targets.len();
    extract_optional_path_keys(map, CHANGE_ITEM_PATH_KEYS, targets, opaque);
    if targets.len() == before {
        *opaque = true;
    }
    extract_change_dest_paths(map, targets, opaque);
}

fn extract_changes_targets(
    changes: &serde_json::Value,
    targets: &mut Vec<String>,
    opaque: &mut bool,
) {
    match changes {
        serde_json::Value::Array(items) if !items.is_empty() => {
            for item in items {
                extract_change_array_item(item, targets, opaque);
            }
        }
        serde_json::Value::Object(map) if !map.is_empty() => {
            for (k, v) in map {
                if k.trim().is_empty() {
                    *opaque = true;
                } else {
                    targets.push(k.clone());
                }
                if let Some(vmap) = v.as_object() {
                    extract_optional_path_keys(vmap, CHANGE_ITEM_PATH_KEYS, targets, opaque);
                    extract_change_dest_paths(vmap, targets, opaque);
                }
            }
        }
        _ => *opaque = true,
    }
}

fn extract_paths_array(
    paths_val: &serde_json::Value,
    targets: &mut Vec<String>,
    opaque: &mut bool,
) {
    let Some(paths) = paths_val.as_array().filter(|a| !a.is_empty()) else {
        *opaque = true;
        return;
    };
    for v in paths {
        match v.as_str().filter(|s| !s.trim().is_empty()) {
            Some(s) => targets.push(s.to_owned()),
            None => *opaque = true,
        }
    }
}

fn extract_concrete_file_targets(request: &ApprovalRequest) -> (Vec<String>, bool) {
    let mut targets = Vec::new();
    let mut opaque = false;
    if let Some(bp) = request
        .blocked_path
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        targets.push(bp.to_owned());
    }
    let Some(obj) = request.input.as_object() else {
        if !request.input.is_null() {
            opaque = true;
        }
        return (targets, opaque);
    };
    const SINGLE_PATH_KEYS: &[&str] = &[
        "file_path",
        "path",
        "TargetFile",
        "AbsolutePath",
        "filePath",
        "target_file",
        "notebook_path",
        "notebookPath",
        "move_path",
        "movePath",
        "new_path",
        "newPath",
        "destination",
    ];
    extract_optional_path_keys(obj, SINGLE_PATH_KEYS, &mut targets, &mut opaque);
    if let Some(paths_val) = obj.get("paths").filter(|v| !v.is_null()) {
        extract_paths_array(paths_val, &mut targets, &mut opaque);
    }
    for changes_key in ["changes", "fileChanges"] {
        if let Some(changes) = obj.get(changes_key).filter(|v| !v.is_null()) {
            extract_changes_targets(changes, &mut targets, &mut opaque);
        }
    }
    (targets, opaque)
}

fn has_concrete_file_target(request: &ApprovalRequest) -> bool {
    let (targets, opaque) = extract_concrete_file_targets(request);
    !targets.is_empty() && !opaque
}

fn request_in_allowed_tools(request: &ApprovalRequest, allowlist: &[ToolName]) -> bool {
    if request.capability == Some(ToolCapability::Unknown) {
        return false;
    }
    allowlist.iter().any(|allowed| {
        let raw = allowed.as_str();
        if ToolCapability::is_capability_token(raw) {
            ToolCapability::parse_rule_token(raw).is_some_and(|cap| request.capability == Some(cap))
        } else {
            raw == request.tool
        }
    })
}

fn rule_matches_tool(rule: &Rule, request: &ApprovalRequest, is_deny: bool) -> bool {
    let rule_tool = rule.tool.as_str();
    if rule_tool == "*" {
        return true;
    }
    if ToolCapability::is_capability_token(rule_tool) {
        return match ToolCapability::parse_rule_token(rule_tool) {
            Some(cap) => {
                request.capability == Some(cap)
                    || (is_deny
                        && cap == ToolCapability::Edit
                        && request.kind == ApprovalKind::FileChange)
            }
            // Fail closed on unrecognized capability tokens: always match denies, never allows.
            None => is_deny,
        };
    }
    rule_tool == request.tool
}

fn has_path_deny(rules: &[Rule], request: &ApprovalRequest) -> bool {
    rules.iter().any(|rule| {
        rule_matches_tool(rule, request, true) && matches!(rule.scope, RuleScope::Path(_))
    })
}

fn has_command_prefix_deny(rules: &[Rule], request: &ApprovalRequest) -> bool {
    rules.iter().any(|rule| {
        rule_matches_tool(rule, request, true) && matches!(rule.scope, RuleScope::CommandPrefix(_))
    })
}

fn is_direct_non_shell_argv(argv: &[String]) -> bool {
    let Some(first) = argv.first() else {
        return false;
    };
    !first.is_empty()
        && !first.contains('=')
        && !is_env_assignment_word(first)
        && !is_shell_binary(first)
}

fn shell_command_is_unambiguously_decoded(input: &serde_json::Value) -> bool {
    let Some(cmd_val) = extract_shell_command(input) else {
        return false;
    };
    match cmd_val {
        serde_json::Value::String(s) => {
            if let Some(outer) = parse_strict_single_command(s, false) {
                if is_shell_binary(&outer[0]) {
                    return outer.len() == 3
                        && is_shell_cmd_flag(&outer[1])
                        && parse_strict_single_command(&outer[2], true)
                            .is_some_and(|inner| is_direct_non_shell_argv(&inner));
                }
            }
            parse_strict_single_command(s, true).is_some_and(|argv| is_direct_non_shell_argv(&argv))
        }
        serde_json::Value::Array(items) => {
            if items.is_empty() {
                return false;
            }
            let Some(argv): Option<Vec<String>> = items
                .iter()
                .map(|v| v.as_str().map(ToOwned::to_owned))
                .collect()
            else {
                return false;
            };
            if is_shell_binary(&argv[0]) {
                return argv.len() == 3
                    && is_shell_cmd_flag(&argv[1])
                    && parse_strict_single_command(&argv[2], true)
                        .is_some_and(|inner| is_direct_non_shell_argv(&inner));
            }
            is_direct_non_shell_argv(&argv) && !argv.iter().any(|arg| has_unsafe_shell_chars(arg))
        }
        _ => false,
    }
}

fn extract_shell_command(input: &serde_json::Value) -> Option<&serde_json::Value> {
    let obj = input.as_object()?;
    let mut found: Option<&serde_json::Value> = None;
    for key in ["command", "cmd", "CommandLine"] {
        if let Some(val) = obj.get(key) {
            if !val.is_null() {
                if found.is_some() {
                    // Fail closed if multiple command keys are present in the same payload.
                    return None;
                }
                found = Some(val);
            }
        }
    }
    found
}

fn is_shell_binary(bin: &str) -> bool {
    matches!(
        bin,
        "sh" | "/bin/sh" | "bash" | "/bin/bash" | "zsh" | "/bin/zsh"
    )
}

fn is_shell_cmd_flag(flag: &str) -> bool {
    matches!(flag, "-c" | "-lc" | "-cl")
}

fn has_unsafe_shell_chars(s: &str) -> bool {
    s.contains("$(")
        || s.chars().any(|c| {
            matches!(
                c,
                ';' | '&' | '|' | '`' | '>' | '<' | '\u{2028}' | '\u{2029}'
            ) || (c.is_ascii_control() && c != '\t')
        })
}

fn is_safe_unquoted_shell_char(c: char) -> bool {
    c.is_ascii_alphanumeric()
        || matches!(c, '-' | '_' | '.' | '/' | ':' | '@' | ',' | '=' | '+' | '%')
}

fn consume_strict_single_quote(
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
    current: &mut String,
    enforce_no_metachars: bool,
) -> bool {
    for qc in chars.by_ref() {
        if qc == '\'' {
            return true;
        }
        if enforce_no_metachars && matches!(qc, ';' | '&' | '|' | '`' | '>' | '<') {
            return false;
        }
        current.push(qc);
    }
    false
}

fn consume_strict_double_quote(
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
    current: &mut String,
    enforce_no_metachars: bool,
) -> bool {
    for qc in chars.by_ref() {
        if qc == '"' {
            return true;
        }
        // Reject every expansion/escape/history character inside double quotes.
        if matches!(qc, '$' | '`' | '\\' | '!') {
            return false;
        }
        if enforce_no_metachars && matches!(qc, ';' | '&' | '|' | '>' | '<') {
            return false;
        }
        current.push(qc);
    }
    false
}

fn has_disallowed_shell_control_char(raw: &str) -> bool {
    raw.chars()
        .any(|c| matches!(c, '\u{2028}' | '\u{2029}') || (c.is_ascii_control() && c != '\t'))
}

fn step_strict_command_char(
    c: char,
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
    words: &mut Vec<String>,
    current: &mut String,
    in_word: &mut bool,
    enforce_no_metachars: bool,
) -> bool {
    match c {
        ' ' | '\t' => {
            if *in_word {
                words.push(std::mem::take(current));
                *in_word = false;
            }
            true
        }
        '\'' => {
            *in_word = true;
            consume_strict_single_quote(chars, current, enforce_no_metachars)
        }
        '"' => {
            *in_word = true;
            consume_strict_double_quote(chars, current, enforce_no_metachars)
        }
        other if is_safe_unquoted_shell_char(other) => {
            *in_word = true;
            current.push(other);
            true
        }
        _ => false,
    }
}

/// Parses one shell command string strictly into literal argv words, rejecting any shell
/// operator, redirection, command/process/parameter/arithmetic substitution (including zsh
/// `=(...)` temporary-file substitution and `*(e:...:)` glob qualifiers), backslash escape,
/// variable expansion, or unterminated quote.
fn parse_strict_single_command(raw: &str, enforce_no_metachars: bool) -> Option<Vec<String>> {
    if has_disallowed_shell_control_char(raw)
        || (enforce_no_metachars && has_unsafe_shell_chars(raw))
    {
        return None;
    }

    let mut words = Vec::new();
    let mut current = String::new();
    let mut in_word = false;
    let mut chars = raw.chars().peekable();

    while let Some(c) = chars.next() {
        if !step_strict_command_char(
            c,
            &mut chars,
            &mut words,
            &mut current,
            &mut in_word,
            enforce_no_metachars,
        ) {
            return None;
        }
    }

    if in_word {
        words.push(current);
    }
    (!words.is_empty()).then_some(words)
}

fn argv_matches_prefix(argv: &[String], prefix: &str) -> bool {
    let prefix_words: Vec<&str> = prefix
        .split([' ', '\t'])
        .filter(|s| !s.is_empty())
        .collect();
    if prefix_words.is_empty() {
        return false;
    }
    let requires_extra_arg = prefix.ends_with([' ', '\t']);
    if requires_extra_arg {
        if argv.len() <= prefix_words.len() {
            return false;
        }
    } else if argv.len() < prefix_words.len() {
        return false;
    }
    argv.iter()
        .zip(prefix_words.iter())
        .all(|(arg, expected)| arg == expected)
}

fn inner_command_str_matches_safe_prefix(raw: &str, prefix: &str) -> bool {
    let Some(argv) = parse_strict_single_command(raw, true) else {
        return false;
    };
    if is_shell_binary(&argv[0]) {
        return false;
    }
    argv_matches_prefix(&argv, prefix)
}

fn command_str_matches_safe_prefix(raw: &str, prefix: &str) -> bool {
    // Allow at most one native shell-wrapper layer (e.g. Codex 0.169.0+ string-serialized
    // `"/bin/zsh -lc 'agent-os inbox'"`), then enforce strict literal-only parsing on the inner
    // invocation.
    if let Some(outer) = parse_strict_single_command(raw, false) {
        if is_shell_binary(&outer[0]) {
            return outer.len() == 3
                && is_shell_cmd_flag(&outer[1])
                && inner_command_str_matches_safe_prefix(&outer[2], prefix);
        }
    }
    inner_command_str_matches_safe_prefix(raw, prefix)
}

fn command_value_matches_safe_prefix(value: &serde_json::Value, prefix: &str) -> bool {
    match value {
        serde_json::Value::String(s) => command_str_matches_safe_prefix(s, prefix),
        serde_json::Value::Array(items) => {
            if items.is_empty() {
                return false;
            }
            let Some(argv): Option<Vec<String>> = items
                .iter()
                .map(|v| v.as_str().map(ToOwned::to_owned))
                .collect()
            else {
                return false;
            };
            if is_shell_binary(&argv[0]) {
                return argv.len() == 3
                    && is_shell_cmd_flag(&argv[1])
                    && inner_command_str_matches_safe_prefix(&argv[2], prefix);
            }
            if argv[0].is_empty() || argv.iter().any(|arg| has_unsafe_shell_chars(arg)) {
                return false;
            }
            argv_matches_prefix(&argv, prefix)
        }
        _ => false,
    }
}

fn is_valid_env_assignment_lhs(lhs: &str) -> bool {
    let name = lhs.strip_suffix('+').unwrap_or(lhs);
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first.is_ascii_alphabetic() || first == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn is_env_assignment_word(word: &str) -> bool {
    word.split_once('=')
        .is_some_and(|(lhs, _)| is_valid_env_assignment_lhs(lhs))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ShellQuoteState {
    Unquoted,
    Single,
    Double,
}

fn push_quoted_segment_char(
    state: &mut ShellQuoteState,
    ch: char,
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
    current: &mut String,
) -> Option<()> {
    if *state == ShellQuoteState::Single {
        if ch == '\'' {
            *state = ShellQuoteState::Unquoted;
        }
        current.push(ch);
        return Some(());
    }
    if ch == '\\' {
        let next = chars.next()?;
        if next == '\n' {
            // POSIX/Bash line continuation inside double quotes removes both `\` and LF.
            return Some(());
        }
        current.push(ch);
        current.push(next);
        return Some(());
    }
    if ch == '"' {
        *state = ShellQuoteState::Unquoted;
    }
    current.push(ch);
    Some(())
}

fn skip_comment_until_lf(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> bool {
    // POSIX/Bash comment runs until LF (`\n`); a bare CR (`\r`) stays inside the comment.
    chars.by_ref().any(|cc| cc == '\n')
}

fn split_unquoted_shell_segments(raw: &str) -> Option<Vec<String>> {
    let mut segments = Vec::new();
    let mut current = String::new();
    let mut quote = ShellQuoteState::Unquoted;
    let mut at_word_start = true;
    let mut chars = raw.chars().peekable();

    while let Some(ch) = chars.next() {
        if quote != ShellQuoteState::Unquoted {
            push_quoted_segment_char(&mut quote, ch, &mut chars, &mut current)?;
            at_word_start = false;
            continue;
        }
        match ch {
            '#' if at_word_start => {
                if skip_comment_until_lf(&mut chars) {
                    if !current.is_empty() {
                        segments.push(std::mem::take(&mut current));
                    }
                    at_word_start = true;
                }
            }
            '\\' => {
                let next = chars.next()?;
                if next != '\n' {
                    current.push(ch);
                    current.push(next);
                    at_word_start = false;
                }
            }
            '\'' => {
                quote = ShellQuoteState::Single;
                current.push(ch);
                at_word_start = false;
            }
            '"' => {
                quote = ShellQuoteState::Double;
                current.push(ch);
                at_word_start = false;
            }
            ' ' | '\t' => {
                current.push(ch);
                at_word_start = true;
            }
            ';' | '&' | '|' | '\n' => {
                if !current.is_empty() {
                    segments.push(std::mem::take(&mut current));
                }
                at_word_start = true;
            }
            _ => {
                current.push(ch);
                at_word_start = false;
            }
        }
    }
    if quote != ShellQuoteState::Unquoted {
        return None;
    }
    if !current.is_empty() {
        segments.push(current);
    }
    Some(segments)
}

fn push_quoted_word_char(
    state: &mut ShellQuoteState,
    ch: char,
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
    current: &mut String,
) -> Option<()> {
    if *state == ShellQuoteState::Single {
        if ch == '\'' {
            *state = ShellQuoteState::Unquoted;
        } else {
            current.push(ch);
        }
        return Some(());
    }
    if ch == '\\' {
        let next = chars.next()?;
        if next == '\n' {
            return Some(());
        }
        if !matches!(next, '"' | '\\' | '$' | '`') {
            current.push('\\');
        }
        current.push(next);
        return Some(());
    }
    if ch == '"' {
        *state = ShellQuoteState::Unquoted;
    } else {
        current.push(ch);
    }
    Some(())
}

fn parse_shell_segment_words(segment: &str) -> Option<Vec<String>> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut in_word = false;
    let mut quote = ShellQuoteState::Unquoted;
    let mut chars = segment.chars().peekable();

    while let Some(ch) = chars.next() {
        if quote != ShellQuoteState::Unquoted {
            push_quoted_word_char(&mut quote, ch, &mut chars, &mut current)?;
            continue;
        }
        match ch {
            ' ' | '\t' => {
                if in_word {
                    words.push(std::mem::take(&mut current));
                    in_word = false;
                }
            }
            '\\' => {
                let next = chars.next()?;
                if next != '\n' {
                    in_word = true;
                    current.push(next);
                }
            }
            '\'' => {
                in_word = true;
                quote = ShellQuoteState::Single;
            }
            '"' => {
                in_word = true;
                quote = ShellQuoteState::Double;
            }
            _ => {
                in_word = true;
                current.push(ch);
            }
        }
    }
    if quote != ShellQuoteState::Unquoted {
        return None;
    }
    if in_word {
        words.push(current);
    }
    (!words.is_empty()).then_some(words)
}

fn decoded_argv_matches_deny_prefix(argv: &[String], prefix: &str) -> bool {
    let start = argv
        .iter()
        .position(|w| !is_env_assignment_word(w))
        .unwrap_or(argv.len());
    let cmd = &argv[start..];
    if cmd.is_empty() {
        return false;
    }
    if cmd.join(" ").trim_start().starts_with(prefix) {
        return true;
    }
    argv_matches_prefix(cmd, prefix)
}

fn advance_quoted_raw_char(
    state: &mut ShellQuoteState,
    i: usize,
    ch: char,
    iter: &mut std::iter::Peekable<std::str::CharIndices<'_>>,
) -> Option<usize> {
    if *state == ShellQuoteState::Single {
        if ch == '\'' {
            *state = ShellQuoteState::Unquoted;
        }
        return Some(i + ch.len_utf8());
    }
    if ch == '\\' {
        let (ni, nc) = iter.next()?;
        return Some(ni + nc.len_utf8());
    }
    if ch == '"' {
        *state = ShellQuoteState::Unquoted;
    }
    Some(i + ch.len_utf8())
}

fn scan_leading_raw_assignment(rest: &str) -> Option<usize> {
    let mut idx = 0;
    let mut eq_pos = None;
    let mut quote = ShellQuoteState::Unquoted;
    let mut iter = rest.char_indices().peekable();
    while let Some((i, ch)) = iter.next() {
        if quote != ShellQuoteState::Unquoted {
            idx = advance_quoted_raw_char(&mut quote, i, ch, &mut iter)?;
            continue;
        }
        match ch {
            ' ' | '\t' => break,
            '\\' => {
                let (ni, nc) = iter.next()?;
                idx = ni + nc.len_utf8();
            }
            '\'' => {
                quote = ShellQuoteState::Single;
                idx = i + 1;
            }
            '"' => {
                quote = ShellQuoteState::Double;
                idx = i + 1;
            }
            '=' if eq_pos.is_none() => {
                eq_pos = Some(i);
                idx = i + 1;
            }
            _ => idx = i + ch.len_utf8(),
        }
    }
    if quote != ShellQuoteState::Unquoted {
        return None;
    }
    let eq = eq_pos?;
    is_valid_env_assignment_lhs(&rest[..eq]).then_some(idx)
}

fn strip_leading_env_assignments_raw(mut rest: &str) -> &str {
    loop {
        rest = rest.trim_start_matches([' ', '\t']);
        let Some(end_idx) = scan_leading_raw_assignment(rest) else {
            return rest;
        };
        rest = &rest[end_idx..];
    }
}

fn decoded_str_matches_deny_prefix(raw: &str, prefix: &str) -> bool {
    let Some(segments) = split_unquoted_shell_segments(raw) else {
        // Undecodable/unbalanced command syntax: do not fabricate false segments;
        // the fail-closed `has_command_prefix_deny` guard escalates it to the owner.
        return false;
    };
    for seg in segments {
        // Check both the raw segment and the raw span after leading `NAME=VALUE` env assignments
        // before quote stripping so literal quoted CommandPrefix rules (e.g. `'rm' -rf` or
        // `"agent-os"`) match verbatim even when preceded by environment assignments.
        let raw_trimmed = seg.trim_start_matches([' ', '\t']);
        if raw_trimmed.starts_with(prefix)
            || strip_leading_env_assignments_raw(raw_trimmed).starts_with(prefix)
        {
            return true;
        }
        let Some(words) = parse_shell_segment_words(&seg) else {
            continue;
        };
        let start = words
            .iter()
            .position(|w| !is_env_assignment_word(w))
            .unwrap_or(words.len());
        let cmd = &words[start..];
        if cmd.len() >= 3
            && is_shell_binary(&cmd[0])
            && is_shell_cmd_flag(&cmd[1])
            && decoded_str_matches_deny_prefix(&cmd[2], prefix)
        {
            return true;
        }
        if decoded_argv_matches_deny_prefix(cmd, prefix) {
            return true;
        }
    }
    false
}

fn matches_command_prefix_deny(input: &serde_json::Value, prefix: &str) -> bool {
    let Some(cmd_val) = extract_shell_command(input) else {
        return false;
    };
    match cmd_val {
        serde_json::Value::String(s) => decoded_str_matches_deny_prefix(s, prefix),
        serde_json::Value::Array(items) => {
            let Some(argv) = items
                .iter()
                .map(|v| v.as_str().map(ToOwned::to_owned))
                .collect::<Option<Vec<String>>>()
            else {
                return false;
            };
            if argv.len() >= 3
                && is_shell_binary(&argv[0])
                && is_shell_cmd_flag(&argv[1])
                && decoded_str_matches_deny_prefix(&argv[2], prefix)
            {
                return true;
            }
            decoded_argv_matches_deny_prefix(&argv, prefix)
        }
        _ => false,
    }
}

fn matches_any_deny(rules: &[Rule], request: &ApprovalRequest, strings: &[String]) -> bool {
    rules.iter().any(|rule| {
        if !rule_matches_tool(rule, request, true) {
            return false;
        }
        match &rule.scope {
            RuleScope::Any => true,
            RuleScope::Path(pattern) => strings
                .iter()
                .any(|s| path_matches_glob_deny(s, pattern.as_str())),
            RuleScope::CommandPrefix(prefix) => {
                matches_command_prefix_deny(&request.input, prefix.as_str())
            }
            RuleScope::McpServer(server) => {
                request
                    .tool
                    .starts_with(&format!("mcp__{}__", server.as_str()))
                    || request
                        .tool
                        .starts_with(&format!("mcp_{}_", server.as_str()))
            }
            _ => true, // Unknown deny scope fails closed.
        }
    })
}

fn matches_any_allow(rules: &[Rule], request: &ApprovalRequest, strings: &[String]) -> bool {
    rules.iter().any(|rule| {
        if !rule_matches_tool(rule, request, false) {
            return false;
        }
        match &rule.scope {
            RuleScope::Any => true,
            RuleScope::Path(pattern) => strings
                .iter()
                .any(|s| path_matches_glob(s, pattern.as_str())),
            RuleScope::CommandPrefix(prefix) => extract_shell_command(&request.input)
                .is_some_and(|cmd| command_value_matches_safe_prefix(cmd, prefix.as_str())),
            RuleScope::McpServer(server) => {
                request
                    .tool
                    .starts_with(&format!("mcp__{}__", server.as_str()))
                    || request
                        .tool
                        .starts_with(&format!("mcp_{}_", server.as_str()))
            }
            _ => false, // Unknown allow scope is dropped.
        }
    })
}

fn path_matches_glob_deny(candidate: &str, pattern: &str) -> bool {
    if org_platform::fs::paths_case_insensitive_by_default() {
        path_matches_glob(&candidate.to_lowercase(), &pattern.to_lowercase())
    } else {
        path_matches_glob(candidate, pattern)
    }
}

fn path_matches_glob(candidate: &str, pattern: &str) -> bool {
    if let Some(prefix) = pattern.strip_suffix("/**") {
        let with_slash = format!("{prefix}/");
        return candidate == prefix
            || candidate.starts_with(&with_slash)
            || candidate.contains(&with_slash)
            || candidate.contains(prefix);
    }
    if let Some(prefix) = pattern.strip_suffix('*') {
        return candidate.starts_with(prefix) || candidate.contains(prefix);
    }
    candidate == pattern || candidate.contains(pattern)
}

fn mode_name(mode: PermissionMode) -> &'static str {
    match mode {
        PermissionMode::Ask => "ask",
        PermissionMode::AcceptEdits => "accept_edits",
        PermissionMode::Plan => "plan",
        _ => "unknown",
    }
}

fn rule_to_view(effect: &str, rule: &Rule) -> GrantRuleView {
    let (scope, pattern) = match &rule.scope {
        RuleScope::Any => ("any", None),
        RuleScope::Path(p) => ("path", Some(p.as_str().to_owned())),
        RuleScope::CommandPrefix(c) => ("command_prefix", Some(c.as_str().to_owned())),
        RuleScope::McpServer(m) => ("mcp_server", Some(m.as_str().to_owned())),
        _ => ("unknown", None),
    };
    GrantRuleView {
        effect: effect.to_owned(),
        tool: rule.tool.as_str().to_owned(),
        scope: scope.to_owned(),
        pattern,
    }
}
