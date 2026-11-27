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

use std::path::Path;

use org_agent_runtime::{
    validate_operator_shell, FileLocation, PlanError, PlannedDir, PlannedFile, RelPath, SessionSpec,
};

/// The project layer's folders, created in the workspace before every launch.
const LAYER_DIRS: [&str; 2] = [".codex", OPERATOR_RULES_DIR];
/// The project layer's config: empty, so nothing but the office's `-c` overrides configures
/// the session.
const LAYER_CONFIG: &str = ".codex/config.toml";
/// The workspace folder Codex loads execpolicy `.rules` files from. The office keeps exactly
/// one file in it, [`OPERATOR_RULES_FILE_NAME`], and removes anything else before a launch.
pub const OPERATOR_RULES_DIR: &str = ".codex/rules";
/// The execpolicy file the office writes for an operator session, in [`OPERATOR_RULES_DIR`].
pub const OPERATOR_RULES_FILE_NAME: &str = "office-operator.rules";
/// [`OPERATOR_RULES_FILE_NAME`] inside [`OPERATOR_RULES_DIR`].
const RULES_FILE: &str = ".codex/rules/office-operator.rules";

/// The folders programs commonly install in. An execpolicy pattern matches the command's
/// first word as typed, so each forbidden program is forbidden by its bare name **and** by its
/// path in each of these (`/bin/bash`, `/usr/bin/env`, `/opt/homebrew/bin/python3`). A path
/// elsewhere still runs only inside the read-only, office-only sandbox (layers 1-3).
pub(crate) const PROGRAM_DIRS: &[&str] = &[
    "/bin",
    "/usr/bin",
    "/usr/local/bin",
    "/opt/homebrew/bin",
    "/sbin",
    "/usr/sbin",
];

/// Interpreters that also install under versioned names, with the versions the execpolicy
/// names (it has no wildcard): `python3.12`, `python2.7`, `pypy3`, `lua5.4`, ...
const VERSIONED_INTERPRETERS: &[(&str, &[&str])] = &[
    (
        "python",
        &[
            "2", "2.7", "3.6", "3.7", "3.8", "3.9", "3.10", "3.11", "3.12", "3.13", "3.14",
        ],
    ),
    ("pypy", &["3"]),
    ("perl", &["5"]),
    ("ruby", &["2", "3"]),
    ("php", &["7", "8"]),
    ("lua", &["5.1", "5.2", "5.3", "5.4"]),
];

/// Interpreter families [`check`] refuses as a prefix under any versioned name, beyond the
/// versions the execpolicy names (`python3.15`, `perl5.40`).
const INTERPRETER_FAMILIES: &[&str] = &["python", "pypy", "perl", "ruby", "php", "lua", "node"];

/// Programs an operator's shell never runs: shells and interpreters (`-c`/`-e` run anything),
/// wrappers that run another command, privilege changers, editors and build tools that run
/// commands, and tools that write files or open connections. Each word is a bare program name
/// (an execpolicy pattern's first token); [`forbidden_words`] adds their paths and versions.
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
    "busybox",
    "toybox",
    "stdbuf",
    "setsid",
    "unbuffer",
    "expect",
    "tclsh",
    "wish",
    "mawk",
    "nawk",
    "gsed",
    "gfind",
    "nodejs",
    "pypy",
    "pip",
    "pip3",
    "pipx",
    "uv",
    "uvx",
    "npm",
    "pnpm",
    "yarn",
    "make",
    "gmake",
    "vi",
    "vim",
    "nvim",
    "ex",
    "ed",
    "emacs",
    "open",
    "launchctl",
    "systemd-run",
    "at",
    "crontab",
    "screen",
    "tmux",
    "chroot",
    "unshare",
    "nsenter",
    "strace",
    "gdb",
    "lldb",
    "docker",
    "podman",
];

/// Whether `word`, a command's first word, names a forbidden program: by its bare name or any
/// path to it (`/bin/bash` is `bash`), or a versioned interpreter (`python3.12`).
pub(crate) fn is_forbidden_program(word: &str) -> bool {
    let name = Path::new(word)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(word);
    let versioned = |family: &&str| {
        name.strip_prefix(family).is_some_and(|version| {
            !version.is_empty() && version.bytes().all(|b| b.is_ascii_digit() || b == b'.')
        })
    };
    FORBIDDEN_PROGRAMS.contains(&name) || INTERPRETER_FAMILIES.iter().any(versioned)
}

/// Every first word the execpolicy forbids: each forbidden program and versioned interpreter
/// by its bare name and by its path in each of [`PROGRAM_DIRS`], once each, in a fixed order.
pub(crate) fn forbidden_words() -> Vec<String> {
    let mut names: Vec<String> = FORBIDDEN_PROGRAMS.iter().map(|p| (*p).to_owned()).collect();
    for (family, versions) in VERSIONED_INTERPRETERS {
        names.extend(versions.iter().map(|version| format!("{family}{version}")));
    }
    let mut words: Vec<String> = Vec::with_capacity(names.len() * (PROGRAM_DIRS.len() + 1));
    for name in &names {
        let paths = PROGRAM_DIRS.iter().map(|dir| format!("{dir}/{name}"));
        for word in std::iter::once(name.clone()).chain(paths) {
            if !words.contains(&word) {
                words.push(word);
            }
        }
    }
    words
}

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
        if is_forbidden_program(words[0]) {
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

/// The execpolicy (Starlark): one `forbidden` rule over [`forbidden_words`] (every forbidden
/// program by name, by path and by version), with a `match` example per word so Codex
/// validates the rule when it loads it, and a `not_match` per operator prefix proving the
/// prefixes stay unmatched (they run sandboxed).
pub(crate) fn rules(spec: &SessionSpec) -> String {
    let quote = |word: &str| serde_json::to_string(word).expect("word encodes");
    let words = forbidden_words();
    let list = |items: &mut dyn Iterator<Item = String>| {
        items
            .map(|item| format!("        {item},\n"))
            .collect::<String>()
    };
    let programs = list(&mut words.iter().map(|w| quote(w)));
    let matches = list(&mut words.iter().map(|w| quote(&format!("{w} x"))));
    let not_matches: Vec<String> = spec
        .operator_shell_prefixes
        .iter()
        .map(|prefix| quote(&format!("{} --help", prefix.as_str().trim_end())))
        .collect();
    format!(
        "# Written by the office for an operator session (TASK-80.1), on every launch.\n\
         # The operator's own commands ({allowed}) have no rule: unmatched, they run inside the\n\
         # read-only, office-socket-only sandbox. An `allow` would run them outside it.\n\
         prefix_rule(\n    pattern = [[\n{programs}    ]],\n    decision = \"forbidden\",\n    \
         justification = \"An operator session runs agent-os only.\",\n    \
         match = [\n{matches}    ],\n    not_match = [{not_matches}],\n)\n",
        allowed = spec
            .operator_shell_prefixes
            .iter()
            .map(|p| p.as_str().trim_end())
            .collect::<Vec<_>>()
            .join(", "),
        not_matches = not_matches.join(", "),
    )
}

#[cfg(test)]
#[path = "operator_tests.rs"]
mod tests;
