//! The spec's permission policy and sandbox, in Claude Code's settings syntax.
//!
//! Claude enforces a restriction in one of two places, and a path restriction needs both:
//! - the **sandbox** (`sandbox.filesystem.*`, `sandbox.network.allowUnixSockets`) confines
//!   the Bash tool, which runs in it;
//! - **permission rules** (`permissions.allow` / `permissions.deny`) hold the file tools
//!   (Read, Grep, Glob, Edit, Write, NotebookEdit), which run in the unsandboxed CLI.
//!
//! Rule syntax: `Tool`, `Tool(specifier)`. A path specifier starting with `//` is absolute
//! (`/x` would be relative to the settings file), `~/` is the home folder, and the rest is
//! gitignore-style globbing. `Read` rules also cover Grep and Glob.

use org_runtime::{
    Capabilities, LaunchContext, PathPattern, PlanError, Rule, RuleScope, SessionSpec,
};
use serde_json::{json, Value};

/// Claude's file-editing tools in 2.1.311 (`init.tools`; there's no `MultiEdit` any more).
/// A path deny for writes names each, rather than relying on `Edit` covering the others.
const EDIT_TOOLS: &[&str] = &["Edit", "Write", "NotebookEdit"];

/// Tools whose path rules are written as `Read(...)` (a `Read` rule covers all three).
const READ_TOOLS: &[&str] = &["Read", "Grep", "Glob"];

/// The shell tool, the only one a command-prefix rule can apply to.
const SHELL_TOOL: &str = "Bash";

/// Reads every Claude agent's tools are denied, whatever the spec says: the user's real
/// `~/.claude` holds every agent's transcripts and the user's own (design decision 6), and
/// `/tmp/cc-socks` holds the messaging sockets of every Claude process on the machine
/// (`/tmp` is a symlink to `/private/tmp` on macOS, so both spellings).
///
/// In gitignore matching `x/**` doesn't match `x` itself (the port's `**` does), so each
/// directory is denied by both spellings.
const FIXED_READ_DENY_RULES: &[&str] = &[
    "Read(~/.claude/**)",
    "Read(~/.claude)",
    "Read(~/.claude.json)",
    "Read(//tmp/cc-socks/**)",
    "Read(//tmp/cc-socks)",
    "Read(//private/tmp/cc-socks/**)",
    "Read(//private/tmp/cc-socks)",
];

/// Tools every agent is denied. `AskUserQuestion` needs content back (its answers travel in
/// `updatedInput`), which the port can't give, so it's declined up front.
const FIXED_DENIED_TOOLS: &[&str] = &["AskUserQuestion"];

/// The user's Claude config, which no file tool may write either (Bash can't: it isn't a
/// writable root). Written as `~/...` rules for each edit tool.
const FIXED_EDIT_DENY_PATHS: &[&str] = &["~/.claude/**", "~/.claude", "~/.claude.json"];

/// Characters that make a shell line more than one command, or quote or expand part of it.
/// Claude splits a compound command and matches each part on its own, so a prefix holding
/// one of these could never match as a whole: a deny with one would be narrower than asked.
const SHELL_METACHARACTERS: &[char] = &[';', '&', '|', '<', '>', '$', '`', '"', '\'', '\n'];

/// The same reads, for the sandbox (`sandbox.filesystem.denyRead`, which expands `~`). A
/// directory denies its whole subtree.
const FIXED_SANDBOX_READ_DENIES: &[&str] = &[
    "~/.claude",
    "~/.claude.json",
    "/tmp/cc-socks",
    "/private/tmp/cc-socks",
];

/// What goes into `settings.json` from the policy and the sandbox.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PolicySettings {
    /// `permissions.allow`.
    pub allow: Vec<String>,
    /// `permissions.deny`.
    pub deny: Vec<String>,
    /// The `sandbox` object.
    pub sandbox: Value,
}

/// Translates the spec's policy, sandbox and read-only dirs, or refuses the spec.
///
/// Enforce, drop or refuse, as [`Capabilities`] sorts them: every restriction Claude can
/// express is enforced exactly or the spec is refused; a grant it can't express exactly is
/// dropped, never widened; allow rules are kept only if [`SessionSpec::keeps_allow_rules`]
/// says so.
pub(crate) fn translate(
    spec: &SessionSpec,
    ctx: &LaunchContext,
    caps: &Capabilities,
) -> Result<PolicySettings, PlanError> {
    let sandbox_on = spec.sandbox.enabled;
    let tmpdir = ctx.tmpdir.as_str();

    // Literal paths go into the sandbox's lists and into gitignore-style rules as they are,
    // and neither syntax has an escape we can rely on, so a path that would read as a glob
    // is refused rather than widened.
    let literal_paths = spec
        .sandbox
        .writable_roots
        .iter()
        .chain(&spec.sandbox.allowed_unix_sockets)
        .chain(&spec.read_only_dirs)
        .map(|p| p.as_str())
        .chain([tmpdir, spec.workspace.as_str()]);
    for path in literal_paths {
        if has_glob_syntax(path) {
            return Err(PlanError::InvalidSpec(format!(
                "the path {path:?} holds rule or glob syntax (one of ( ) * ? [ ] {{ }} \\), \
                 which Claude's sandbox and rules would read as more than the path"
            )));
        }
    }

    // The writable roots, the session's TMPDIR always among them.
    let mut writable: Vec<&str> = spec
        .sandbox
        .writable_roots
        .iter()
        .map(|p| p.as_str())
        .collect();
    writable.push(tmpdir);
    let writable = dedup(writable);

    // A read-only dir gets a deny on every edit tool (below). One that holds a writable root
    // (or the workspace, or TMPDIR) is a contradiction: the deny would take the root away
    // from the file tools while Bash could still write it. Refuse it.
    for dir in &spec.read_only_dirs {
        let inner = writable
            .iter()
            .copied()
            .chain([spec.workspace.as_str()])
            .find(|path| within(path, dir.as_str()));
        if let Some(inner) = inner {
            return Err(PlanError::InvalidSpec(format!(
                "the read-only dir {:?} holds {inner:?}, which must stay writable",
                dir.as_str()
            )));
        }
    }

    // Claude's sandbox makes its cwd writable by itself, and acceptEdits accepts edits in
    // every working directory (the cwd and each --add-dir) without asking. So while the
    // writable roots are enforced, the cwd must be inside one, or Claude would make a
    // writable place the spec didn't ask for. (The --add-dir folders are the read-only
    // dirs, denied below.)
    if sandbox_on
        && !writable
            .iter()
            .any(|root| within(spec.workspace.as_str(), root))
    {
        return Err(PlanError::InvalidSpec(
            "the workspace must be inside a writable root while the sandbox is on".to_owned(),
        ));
    }

    let mut deny: Vec<String> = FIXED_READ_DENY_RULES
        .iter()
        .chain(FIXED_DENIED_TOOLS)
        .map(|r| r.to_string())
        .collect();
    for tool in EDIT_TOOLS {
        deny.extend(FIXED_EDIT_DENY_PATHS.iter().map(|p| format!("{tool}({p})")));
    }
    let mut deny_read: Vec<String> = Vec::new();
    let mut deny_write: Vec<String> = Vec::new();

    // Read-only dirs: never written, by the file tools (rules) or by Bash (the sandbox,
    // explicitly, in case Claude adds its --add-dir folders to its own write list).
    for dir in &spec.read_only_dirs {
        deny.extend(edit_rules(&format!("{}/**", dir.as_str())));
        if sandbox_on {
            deny_write.push(dir.as_str().to_owned());
        }
    }

    for rule in &spec.policy.deny {
        let parts = deny_rule(rule, sandbox_on)?;
        deny.extend(parts.rules);
        deny_read.extend(parts.sandbox_read);
        deny_write.extend(parts.sandbox_write);
    }

    if sandbox_on {
        for pattern in &spec.sandbox.deny_write {
            deny.extend(edit_rules(pattern.as_str()));
            deny_write.extend(sandbox_paths(pattern));
        }
        for pattern in &spec.sandbox.deny_read {
            deny.extend(deny_path_rules("Read", pattern.as_str()));
            deny_read.extend(sandbox_paths(pattern));
        }
    }

    let mut allow = Vec::new();
    if spec.keeps_allow_rules(caps) {
        for rule in &spec.policy.allow {
            allow.extend(allow_rule(rule, sandbox_on.then_some(&writable[..])));
        }
    }

    let sandbox = if sandbox_on {
        let mut read: Vec<String> = FIXED_SANDBOX_READ_DENIES
            .iter()
            .map(|p| p.to_string())
            .collect();
        read.extend(deny_read);
        json!({
            "enabled": true,
            "autoAllowBashIfSandboxed": false,
            "allowUnsandboxedCommands": false,
            "filesystem": {
                "allowWrite": writable,
                "denyWrite": dedup(deny_write),
                "denyRead": dedup(read),
            },
            "network": {
                "allowUnixSockets": dedup(
                    spec.sandbox.allowed_unix_sockets.iter().map(|p| p.as_str()).collect()
                ),
            },
        })
    } else {
        json!({"enabled": false})
    };

    Ok(PolicySettings {
        allow: dedup(allow),
        deny: dedup(deny),
        sandbox,
    })
}

/// One deny rule's translation.
#[derive(Default)]
struct DenyParts {
    rules: Vec<String>,
    sandbox_read: Vec<String>,
    sandbox_write: Vec<String>,
}

/// A deny rule, exactly, or a refusal. It may come out broader than asked (a `Grep` path
/// deny becomes a `Read` deny, which covers Grep and Glob too), never narrower.
fn deny_rule(rule: &Rule, sandbox_on: bool) -> Result<DenyParts, PlanError> {
    let tool = rule.tool.as_str();
    let refuse = |why: &str| {
        Err(PlanError::InvalidSpec(format!(
            "a deny rule for {tool:?} {why}, which Claude can't enforce exactly"
        )))
    };
    let mut parts = DenyParts::default();
    match &rule.scope {
        RuleScope::Any => {
            if tool == "*" {
                return refuse("on every use of every tool");
            }
            parts.rules.push(tool.to_owned());
        }
        RuleScope::CommandPrefix(prefix) => {
            if tool != "*" && tool != SHELL_TOOL {
                return refuse("with a command prefix on a tool that isn't the shell");
            }
            if has_shell_metacharacters(prefix.as_str()) {
                return refuse("with a command prefix holding shell operators or quotes");
            }
            parts.rules.push(prefix_rule(prefix.as_str()));
        }
        RuleScope::McpServer(server) => match mcp_rule(tool, server.as_str()) {
            Some(r) => parts.rules.push(r),
            None => return refuse("with an MCP server scope that doesn't name that server"),
        },
        RuleScope::Path(pattern) => {
            let p = pattern.as_str();
            let bash = |parts: &mut DenyParts| {
                parts.sandbox_read.extend(sandbox_paths(pattern));
                parts.sandbox_write.extend(sandbox_paths(pattern));
            };
            if tool == "*" || tool == SHELL_TOOL {
                // Only the sandbox holds Bash to a path.
                if !sandbox_on {
                    return refuse("on a path while the sandbox is off");
                }
                bash(&mut parts);
                if tool == "*" {
                    parts.rules.extend(deny_path_rules("Read", p));
                    parts.rules.extend(edit_rules(p));
                }
            } else if READ_TOOLS.contains(&tool) {
                parts.rules.extend(deny_path_rules("Read", p));
            } else if EDIT_TOOLS.contains(&tool) || tool == "MultiEdit" {
                parts.rules.extend(edit_rules(p));
            } else {
                return refuse("on a path, for a tool that takes no path rules");
            }
        }
        _ => return refuse("with a scope this adapter doesn't know"),
    }
    Ok(parts)
}

/// An allow rule, or nothing when Claude can't express it exactly (dropping a grant only
/// makes the session ask more). `writable` is the writable roots while they're enforced:
/// then an edit grant outside them is dropped, because writability never comes from a grant.
fn allow_rule(rule: &Rule, writable: Option<&[&str]>) -> Vec<String> {
    let tool = rule.tool.as_str();
    let edit_ok = |p: &str| writable.is_none_or(|roots| roots.iter().any(|r| within(p, r)));
    match &rule.scope {
        // `*` has no Claude spelling; a bare edit tool would grant edits anywhere.
        RuleScope::Any if tool == "*" => vec![],
        RuleScope::Any if writable.is_some() && is_edit_tool(tool) => vec![],
        RuleScope::Any => vec![tool.to_owned()],
        // `Bash(foo:*)` is Claude's legacy prefix syntax for `foo`, which is wider than the
        // literal prefix `foo:`, so such a grant is dropped.
        RuleScope::CommandPrefix(prefix) if prefix.as_str().ends_with(':') => vec![],
        // Matched part by part by Claude: not the literal prefix it says.
        RuleScope::CommandPrefix(prefix) if has_shell_metacharacters(prefix.as_str()) => vec![],
        RuleScope::CommandPrefix(prefix) if tool == "*" || tool == SHELL_TOOL => {
            vec![prefix_rule(prefix.as_str())]
        }
        RuleScope::CommandPrefix(_) => vec![],
        RuleScope::McpServer(server) => mcp_rule(tool, server.as_str()).into_iter().collect(),
        // In gitignore matching, a pattern that matches a directory grants everything under
        // it too, so only a pattern that already covers a whole subtree (ending in `/**`)
        // grants exactly what it says.
        RuleScope::Path(pattern) if !pattern.as_str().ends_with("/**") => vec![],
        RuleScope::Path(pattern) => {
            let p = pattern.as_str();
            let mut rules = Vec::new();
            if tool == "Read" || tool == "*" {
                rules.push(path_rule("Read", p));
            }
            // `Edit(...)` grants every edit tool, so only an `Edit` or `*` grant maps to it.
            if (tool == "Edit" || tool == "*") && edit_ok(p) {
                rules.push(path_rule("Edit", p));
            }
            rules
        }
        _ => vec![],
    }
}

fn is_edit_tool(tool: &str) -> bool {
    EDIT_TOOLS.contains(&tool) || tool == "MultiEdit"
}

/// `Tool(//abs/path)`: `//` makes the path absolute in Claude's rule syntax.
fn path_rule(tool: &str, pattern: &str) -> String {
    format!("{tool}(/{pattern})")
}

/// A path deny for `tool`: `X/**` is also written as `X`, because gitignore's `X/**`
/// doesn't match `X` itself while the port's `**` does (a deny may come out broader, never
/// narrower).
fn deny_path_rules(tool: &str, pattern: &str) -> Vec<String> {
    let mut rules = vec![path_rule(tool, pattern)];
    if let Some(dir) = pattern.strip_suffix("/**").filter(|d| !d.is_empty()) {
        rules.push(path_rule(tool, dir));
    }
    rules
}

/// A deny of every edit tool on `pattern`.
fn edit_rules(pattern: &str) -> Vec<String> {
    EDIT_TOOLS
        .iter()
        .flat_map(|t| deny_path_rules(t, pattern))
        .collect()
}

/// `Bash(<prefix>*)`: commands that start with the literal prefix.
fn prefix_rule(prefix: &str) -> String {
    format!("{SHELL_TOOL}({prefix}*)")
}

/// An MCP rule: `mcp__<server>` (every tool of the server) for `*` or that name itself, or
/// one of the server's tools by its full name.
///
/// A server name holding `__` is refused (`None`): Claude would read `mcp__a__b` as the tool
/// `b` of the server `a`.
fn mcp_rule(tool: &str, server: &str) -> Option<String> {
    if server.contains("__") {
        return None;
    }
    let all = format!("mcp__{server}");
    if tool == "*" || tool == all {
        Some(all)
    } else if tool.starts_with(&format!("{all}__")) {
        Some(tool.to_owned())
    } else {
        None
    }
}

/// A pattern in the sandbox's path syntax: `X/**` (with no other wildcard) is the directory
/// `X`, which covers its whole subtree, `X` itself included, as `**` does; a pattern with no
/// wildcard is its literal path; anything else is passed as a glob, and a glob `X/**` also
/// as `X`, so the directory itself is covered too.
///
/// An interior `**` is also written collapsed (`/a/**/b` as `/a/b` too), in every
/// combination, in case the sandbox's `**` needs at least one segment, as the port's
/// doesn't.
pub(crate) fn sandbox_paths(pattern: &PathPattern) -> Vec<String> {
    let mut out = Vec::new();
    for variant in collapsed_variants(pattern.as_str()) {
        let p = variant.as_str();
        match p.strip_suffix("/**") {
            Some("") => out.push("/".to_owned()),
            Some(dir) if !dir.contains('*') => out.push(dir.to_owned()),
            Some(dir) => {
                out.push(p.to_owned());
                out.push(dir.to_owned());
            }
            None => out.push(p.to_owned()),
        }
    }
    dedup(out)
}

/// `pattern`, and every version of it with some of its interior `**` segments removed.
fn collapsed_variants(pattern: &str) -> Vec<String> {
    let segments: Vec<&str> = pattern.split('/').skip(1).collect();
    let interior: Vec<usize> = (0..segments.len().saturating_sub(1))
        .filter(|&i| segments[i] == "**")
        .collect();
    (0u32..1 << interior.len())
        .map(|mask| {
            let kept: Vec<&str> = segments
                .iter()
                .enumerate()
                .filter(|(i, _)| {
                    interior
                        .iter()
                        .position(|j| j == i)
                        .is_none_or(|bit| mask & (1 << bit) == 0)
                })
                .map(|(_, s)| *s)
                .collect();
            format!("/{}", kept.join("/"))
        })
        .collect()
}

/// Whether `path` (a literal path or a pattern whose wildcards are whole segments) is
/// `root` or inside it. `root` is a literal path.
fn within(path: &str, root: &str) -> bool {
    root == "/"
        || path == root
        || path
            .strip_prefix(root)
            .is_some_and(|rest| rest.starts_with('/'))
}

fn has_shell_metacharacters(prefix: &str) -> bool {
    prefix.contains(SHELL_METACHARACTERS)
}

fn has_glob_syntax(path: &str) -> bool {
    path.contains(['(', ')', '*', '?', '[', ']', '{', '}', '\\'])
}

/// Removes repeats, keeping the first of each.
fn dedup<T: PartialEq + Clone>(items: Vec<T>) -> Vec<T> {
    let mut out: Vec<T> = Vec::with_capacity(items.len());
    for item in items {
        if !out.contains(&item) {
            out.push(item);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pat(p: &str) -> PathPattern {
        PathPattern::try_from(p).unwrap()
    }

    #[test]
    fn sandbox_path_forms() {
        assert_eq!(sandbox_paths(&pat("/a/b/**")), ["/a/b"]);
        assert_eq!(sandbox_paths(&pat("/**")), ["/"]);
        assert_eq!(sandbox_paths(&pat("/a/*/c/**")), ["/a/*/c/**", "/a/*/c"]);
        assert_eq!(sandbox_paths(&pat("/a/b")), ["/a/b"]);
        assert_eq!(sandbox_paths(&pat("/a/*/db")), ["/a/*/db"]);
        assert_eq!(sandbox_paths(&pat("/a/**/b")), ["/a/**/b", "/a/b"]);
        assert_eq!(
            sandbox_paths(&pat("/a/**/b/**/c/**")),
            [
                "/a/**/b/**/c/**",
                "/a/**/b/**/c",
                "/a/b/**/c/**",
                "/a/b/**/c",
                "/a/**/b/c/**",
                "/a/**/b/c",
                "/a/b/c"
            ]
        );
    }

    #[test]
    fn within_is_by_whole_segments() {
        assert!(within("/a/b", "/a"));
        assert!(within("/a", "/a"));
        assert!(within("/a/**", "/a"));
        assert!(!within("/ab", "/a"));
        assert!(!within("/a", "/a/b"));
        assert!(within("/anything", "/"));
    }

    #[test]
    fn rule_spellings() {
        assert_eq!(path_rule("Read", "/x/**"), "Read(//x/**)");
        assert_eq!(
            deny_path_rules("Read", "/x/**"),
            ["Read(//x/**)", "Read(//x)"]
        );
        assert_eq!(deny_path_rules("Read", "/**"), ["Read(//**)"]);
        assert_eq!(deny_path_rules("Read", "/x/f"), ["Read(//x/f)"]);
        assert_eq!(prefix_rule("git "), "Bash(git *)");
        assert_eq!(mcp_rule("*", "office").as_deref(), Some("mcp__office"));
        assert_eq!(
            mcp_rule("mcp__office__send", "office").as_deref(),
            Some("mcp__office__send")
        );
        assert_eq!(mcp_rule("mcp__officex__send", "office"), None);
        assert_eq!(mcp_rule("Bash", "office"), None);
        assert_eq!(mcp_rule("*", "a__b"), None);
        assert!(has_shell_metacharacters("cd /x && rm"));
        assert!(has_shell_metacharacters("echo 'x"));
        assert!(!has_shell_metacharacters("git status"));
    }
}
