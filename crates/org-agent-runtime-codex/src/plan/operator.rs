//! Codex's shell for an operator session (`OperatorSandboxed`, `TASK-80.1`, `doc-97` §3.1).
//!
//! Codex runs shell commands without the office's approval bridge, so a shell scoped to a
//! command prefix turns Codex's shell tool off (`features.shell_tool=false`) and an ordinary
//! `ReadOnly` agent keeps it off. That locked the Chief of Staff out of `agent-os`. An operator
//! session ([`SessionSpec::operator_shell_prefixes`]) turns the shell **on** instead, confined
//! four ways, each of which holds without the others:
//!
//! 1. **A read-only sandbox with no writable root** (not even the workspace or `TMPDIR`): the
//!    spec has none ([`validate_operator_shell`]) and the plan refuses any sandbox mode but
//!    `read-only`, so no command writes anything.
//! 2. **No network but the office socket** (the office `permissions` profile).
//! 3. **No MCP server** and the office's empty `mcp_servers` table.
//! 4. **An execpolicy that forbids every shell, interpreter and wrapper** that could run
//!    another command past `agent-os` (`bash -c`, `env`, `xargs`, `python`, ...), and the
//!    tools that write or reach out (`rm`, `tee`, `curl`, `git`, ...). Codex splits a compound
//!    command (`agent-os x && rm y`) and judges each part, so a forbidden part blocks it.
//!
//! The operator's own prefixes get **no** rule on purpose: an execpolicy `allow` runs the
//! command *outside* the sandbox without asking, which would hand `agent-os` the user's whole
//! disk. Unmatched, `agent-os` runs inside the sandbox and reaches the office over its socket,
//! as on Claude and Antigravity. Codex's execpolicy has no catch-all rule, so a command that is
//! neither `agent-os` nor forbidden still runs, but only inside the read-only, office-only
//! sandbox, where it can read what the agent's `cap:read` already reads and nothing more.
//!
//! The rules load from the workspace's project layer (`.codex/rules/`), which Codex reads only
//! for a trusted project: the plan trusts the workspace (the agent's repo-less private home)
//! and rewrites the layer's files on every launch, so no stale project config survives.

use org_agent_runtime::{
    validate_operator_shell, FileLocation, PlanError, PlannedDir, PlannedFile, RelPath, SessionSpec,
};

/// The project layer's folders, created in the workspace before every launch.
const LAYER_DIRS: [&str; 2] = [".codex", ".codex/rules"];
/// The project layer's config: empty, so nothing but the office's `-c` overrides configures
/// the session.
const LAYER_CONFIG: &str = ".codex/config.toml";
/// The execpolicy the office writes for an operator session.
const RULES_FILE: &str = ".codex/rules/office-operator.rules";

/// Programs an operator's shell never runs: shells and interpreters (`-c`/`-e` run anything),
/// wrappers that run another command, privilege changers, and tools that write files or open
/// connections. Each word is a bare program name (an execpolicy pattern's first token).
pub(crate) const FORBIDDEN_PROGRAMS: &[&str] = &[
    "bash",
    "sh",
    "zsh",
    "dash",
    "fish",
    "ksh",
    "csh",
    "tcsh",
    "pwsh",
    "env",
    "xargs",
    "nohup",
    "timeout",
    "nice",
    "exec",
    "eval",
    "command",
    "builtin",
    "time",
    "watch",
    "script",
    "sudo",
    "su",
    "doas",
    "python",
    "python3",
    "perl",
    "ruby",
    "node",
    "deno",
    "bun",
    "npx",
    "php",
    "lua",
    "osascript",
    "awk",
    "gawk",
    "sed",
    "find",
    "curl",
    "wget",
    "nc",
    "ncat",
    "socat",
    "telnet",
    "ssh",
    "scp",
    "sftp",
    "rsync",
    "git",
    "rm",
    "rmdir",
    "mv",
    "cp",
    "dd",
    "tee",
    "touch",
    "mkdir",
    "chmod",
    "chown",
    "ln",
    "install",
    "truncate",
    "kill",
    "pkill",
    "killall",
];

/// Whether `spec` is an operator session whose shell this adapter turns on. A malformed
/// operator spec is not one: [`check`] refuses it before anything is planned.
pub(crate) fn confines_shell(spec: &SessionSpec) -> bool {
    validate_operator_shell(spec).unwrap_or(false)
}

/// Checks an operator session for Codex: the port's rules, a `read-only` sandbox mode, and
/// prefixes that are plain program words. Answers whether `spec` is one.
pub(crate) fn check(spec: &SessionSpec, sandbox_mode: &str) -> Result<bool, PlanError> {
    if !validate_operator_shell(spec)? {
        return Ok(false);
    }
    if sandbox_mode != "read-only" {
        return Err(PlanError::InvalidSpec(
            "operator session: Codex runs it only in a read-only sandbox".to_owned(),
        ));
    }
    for prefix in &spec.operator_shell_prefixes {
        let words: Vec<&str> = prefix.as_str().split_whitespace().collect();
        let plain = |w: &&str| {
            w.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-/".contains(&b))
        };
        if words.is_empty() || !words.iter().all(plain) {
            return Err(PlanError::InvalidSpec(
                "operator session: a shell prefix is plain program words".to_owned(),
            ));
        }
        if FORBIDDEN_PROGRAMS.contains(&words[0]) {
            return Err(PlanError::InvalidSpec(
                "operator session: a shell prefix names a forbidden program".to_owned(),
            ));
        }
    }
    Ok(true)
}

/// The `-c` override that trusts the workspace, so Codex loads its project layer (and so the
/// office's rules). The workspace is the agent's repo-less private home.
pub(crate) fn trust_override(spec: &SessionSpec) -> String {
    let workspace = serde_json::to_string(spec.workspace.as_str()).expect("path encodes");
    format!("projects={{{workspace}={{trust_level=\"trusted\"}}}}")
}

/// The project layer's folders and files, written into the workspace before every launch.
pub(crate) fn layer(spec: &SessionSpec) -> (Vec<PlannedDir>, Vec<PlannedFile>) {
    let dirs = LAYER_DIRS
        .iter()
        .map(|dir| PlannedDir {
            location: FileLocation::Workspace,
            rel_path: RelPath::try_from(*dir).expect("static rel path"),
        })
        .collect();
    let file = |path: &str, contents: String| PlannedFile {
        location: FileLocation::Workspace,
        rel_path: RelPath::try_from(path).expect("static rel path"),
        contents: contents.into_bytes(),
        git_exclude: false,
    };
    let config = "# Written by the office on every launch: the session is configured by its \
                  command line alone.\n"
        .to_owned();
    let files = vec![file(LAYER_CONFIG, config), file(RULES_FILE, rules(spec))];
    (dirs, files)
}

/// The execpolicy (Starlark): one `forbidden` rule over [`FORBIDDEN_PROGRAMS`], with a
/// `match` example per program so Codex validates the rule when it loads it, and a
/// `not_match` per operator prefix proving the prefixes stay unmatched (they run sandboxed).
pub(crate) fn rules(spec: &SessionSpec) -> String {
    let quote = |word: &str| serde_json::to_string(word).expect("word encodes");
    let programs: Vec<String> = FORBIDDEN_PROGRAMS.iter().map(|p| quote(p)).collect();
    let matches: Vec<String> = FORBIDDEN_PROGRAMS
        .iter()
        .map(|p| quote(&format!("{p} x")))
        .collect();
    let not_matches: Vec<String> = spec
        .operator_shell_prefixes
        .iter()
        .map(|prefix| quote(&format!("{} --help", prefix.as_str().trim_end())))
        .collect();
    format!(
        "# Written by the office for an operator session (TASK-80.1), on every launch.\n\
         # The operator's own commands ({allowed}) have no rule: unmatched, they run inside the\n\
         # read-only, office-socket-only sandbox. An `allow` would run them outside it.\n\
         prefix_rule(\n    pattern = [[{programs}]],\n    decision = \"forbidden\",\n    \
         justification = \"An operator session runs agent-os only.\",\n    \
         match = [{matches}],\n    not_match = [{not_matches}],\n)\n",
        allowed = spec
            .operator_shell_prefixes
            .iter()
            .map(|p| p.as_str().trim_end())
            .collect::<Vec<_>>()
            .join(", "),
        programs = programs.join(", "),
        matches = matches.join(", "),
        not_matches = not_matches.join(", "),
    )
}
