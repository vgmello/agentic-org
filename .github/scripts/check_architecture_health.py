#!/usr/bin/env python3
"""Automated CI Architecture Health & Shrink-Only Coupling Guardrails (TASK-234 / P0-INT-4).

Enforces three mechanical workspace invariants plus shrink-only allowlist verification
in pure Python 3 stdlib (zero third-party dependencies, < 2s full-repo execution):

1. Crate Size Ratchet (Anti-Monolith Gate):
   - Counts non-test production `.rs` LOC per workspace crate (`crates/*`, `apps/*`).
   - Deterministic counting rule:
     a) Dedicated test files and directories are excluded: any `.rs` file whose path
        relative to its crate root (with trailing `.rs` stripped) contains a component
        named `tests`, `testing`, `testkit`, or `test_support`, starting with `testkit`,
        or ending with `_tests` or `-tests`.
     b) Inline `#[cfg(test)]` and `#[cfg(all(..., test, ...))]` items/blocks (where
        `test` is a direct conjunct, never `not(test)` or `any(test, ...)`) are stripped
        using a Rust-aware lexical scanner (handling line comments, nested block
        comments, normal/byte/c/raw string literals, char literals, chained outer
        attributes, balanced delimiters, `let`/`if ... else` statements, and struct
        fields) before counting remaining lines.
     c) Lines wholly covered by stripped test items (including blank/comment lines
        inside those blocks) are excluded; all other production lines (code, comments,
        blank lines in production sections) are counted identically to `wc -l` (plus an
        unterminated final line).
   - Unlisted crate >= 25,000 non-test LOC: emits a non-fatal warning.
   - Unlisted crate > 30,000 non-test LOC: fails CI (exit 1).
   - Listed legacy large crates in `[crate_loc]` of `.github/architecture-boundaries.allow`:
     must stay at or below their shrink-only ceiling (`actual <= ceiling`).

2. Shrink-Only Infrastructure Coupling Allowlist & Port `#[allow(dead_code)]` Ban:
   - Scans production `.rs` files (same test exclusions and `#[cfg(test)]` stripping):
     a) `coupling.rusqlite`: `rusqlite` outside `crates/org-office-sqlite/src/`
        and `crates/org-host-sqlite/src/`.
     b) `coupling.unix_socket`: `UnixStream` / `UnixListener` / `UnixDatagram` /
        `std::os::unix::net` / `tokio::net::Unix*` outside `crates/org-platform/`,
        `crates/org-platform-host/`, `crates/org-socket/`, `crates/org-client/`, and
        `apps/agentic-org-web/src/transport/local_unix.rs`.
     c) `coupling.sandbox`: `bwrap` / `sandbox-exec` process invocation string literals
        or concrete sandbox/bwrap/Seatbelt FFI, type, function, or module identifiers
        (`sandbox_init`, `sandbox_init_with_parameters`, `sandbox_check`, `sandbox_exec`,
        or any identifier containing `bwrap` or `seatbelt` case-insensitively) outside
        designated local OS/runtime sandbox adapters (`crates/org-platform/`,
        `crates/org-platform-host/`, `crates/org-office-sessions/src/manager/sandbox_runner/`,
        and `crates/org-agent-runtime-claude/src/plan/sandbox.rs`).
     d) `coupling.dead_code_trait`: `#[allow(...dead_code...)]` / `#[expect(...dead_code...)]`
        (including via `#[cfg_attr(..., allow|expect(...dead_code...))]`) directly
        preceding any `trait` definition across all crates.
   - Enforces exact shrink-only agreement with `.github/architecture-boundaries.allow`:
     unlisted hits fail, raised counts fail, and reduced counts must be lowered (or
     deleted at 0) in the same change so wins stay permanently locked in.

3. Workspace Cargo DAG & Layer-0 Foundation Stability Guard:
   - Parses root `Cargo.toml` workspace members (plus `[workspace.dependencies]` rename
     aliases) and each member's normal `[dependencies]`, `[build-dependencies]`,
     `[target.*.dependencies]`, and `[target.*.build-dependencies]` (cleanly excluding
     `[dev-dependencies]`).
   - Verifies 0 cyclic dependencies across all workspace crates (reporting the exact
     cycle path on failure).
   - Verifies Layer-0 foundation crates (`org-proto`, `org-platform`, `org-pki`,
     `org-redact`, `org-store`, `org-agent-runtime`, `org-command`,
     `org-command-catalogue`, `org-sync-proto`, `org-host-control`,
     `org-office-launch`, plus zero-dep leaf `org-docs` used by `org-command`
     for bounded YAML parsing) have zero `[dependencies]` or `[build-dependencies]`
     edges to any higher-layer application/server/host/office/CLI crate.

4. Shrink-Only Base Diff (`--base <commit-sha>`):
   - Compares `.github/architecture-boundaries.allow` at `<base>` with the working tree.
   - `[crate_loc]`: rejects adding any crate, raising any ceiling, or raising the total.
   - `[coupling.*]`: rejects raising any listed path's count or raising any section
     total (a new path is accepted only if the section total does not rise and no
     existing path rises, permitting file renames/splits without adding coupling).

Usage:
  python3 .github/scripts/check_architecture_health.py
  python3 .github/scripts/check_architecture_health.py --base <commit-sha>
  python3 .github/scripts/check_architecture_health.py --print-baseline
"""

from __future__ import annotations

import argparse
import os
import re
import subprocess
import sys
import time
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

_SCRIPT_DIR = str(Path(__file__).resolve().parent)
if _SCRIPT_DIR not in sys.path:
    sys.path.insert(0, _SCRIPT_DIR)

from architecture_hygiene_checks import check_workspace_hygiene

tomllib: Any
try:
    import tomllib as _stdlib_tomllib
except ModuleNotFoundError:
    tomllib = None
else:
    tomllib = _stdlib_tomllib


ALLOWLIST_REL = ".github/architecture-boundaries.allow"
RULES_DOC = ".agents/rules/architecture-and-layout.md"

CRATE_WARN_LOC = 25_000
CRATE_FAIL_LOC = 30_000

SECTION_CRATE_LOC = "crate_loc"
SECTION_RUSQLITE = "coupling.rusqlite"
SECTION_UNIX_SOCKET = "coupling.unix_socket"
SECTION_SANDBOX = "coupling.sandbox"
SECTION_DEAD_CODE_TRAIT = "coupling.dead_code_trait"

COUPLING_SECTIONS = (
    SECTION_RUSQLITE,
    SECTION_UNIX_SOCKET,
    SECTION_SANDBOX,
    SECTION_DEAD_CODE_TRAIT,
)
ALL_SECTIONS = (SECTION_CRATE_LOC, *COUPLING_SECTIONS)

# Designated adapter directory prefixes (repo-relative POSIX paths).
RUSQLITE_ADAPTER_PREFIXES = (
    "crates/org-office-sqlite/src/",
    "crates/org-host-sqlite/src/",
)
UNIX_SOCKET_ADAPTER_PREFIXES = (
    "crates/org-platform/",
    "crates/org-platform-host/",
    "crates/org-socket/",
    "crates/org-client/",
    "apps/agentic-org-web/src/transport/local_unix.rs",
)
SANDBOX_ADAPTER_PREFIXES = (
    "crates/org-platform/",
    "crates/org-platform-host/",
    "crates/org-office-sessions/src/manager/sandbox_runner/",
    "crates/org-agent-runtime-claude/src/plan/sandbox.rs",
)

# Layer-0 foundation crates (dynamically intersected with actual workspace members).
# `org-docs` is a zero-workspace-dep pure Markdown/YAML leaf used by `org-command`
# for `--input-format yaml` and is guarded at Layer-0 too so it cannot create a
# transitive upward edge from `org-command`.
LAYER_0_FOUNDATION_CRATES = frozenset(
    {
        "org-proto",
        "org-platform",
        "org-pki",
        "org-redact",
        "org-store",
        "org-agent-runtime",
        "org-command",
        "org-command-catalogue",
        "org-sync-proto",
        "org-host-control",
        "org-office-launch",
    }
)
LAYER_0_LEAF_CRATES = frozenset({"org-docs"})
LAYER_0_ALLOWED_WORKSPACE_DEPS = LAYER_0_FOUNDATION_CRATES | LAYER_0_LEAF_CRATES

# Dedicated test file/directory names (aligned with file_size_ratchet.rs + testkit*).
TEST_EXACT_NAMES = frozenset({"tests", "testing", "testkit", "test_support"})
TEST_PREFIXES = ("testkit",)
TEST_SUFFIXES = ("_tests", "-tests")

# Git repository env vars cleared when invoking `git -C` so tests run under hooks or
# worktrees never query the parent repository by accident.
GIT_REPO_ENV_VARS = (
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_COMMON_DIR",
    "GIT_DIR",
    "GIT_GRAFT_FILE",
    "GIT_IMPLICIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_NO_REPLACE_OBJECTS",
    "GIT_OBJECT_DIRECTORY",
    "GIT_PREFIX",
    "GIT_REPLACE_REF_BASE",
    "GIT_SHALLOW_FILE",
    "GIT_WORK_TREE",
)

# Directories skipped during `.rs` source walk inside a crate.
SKIPPED_DIRS = frozenset({"target", "node_modules", ".git"})

# Fast pre-filter tokens: if NONE appear in a file, it has neither #[cfg(test)] nor any
# tracked coupling primitive, allowing O(1) newline counting without lexical masking.
FAST_PATH_TRIGGERS = (
    "cfg",
    "rusqlite",
    "Unix",
    "unix::net",
    "bwrap",
    "Bwrap",
    "BWRAP",
    "sandbox",
    "seatbelt",
    "Seatbelt",
    "SEATBELT",
    "dead_code",
)

_LEX_MARKERS = ("//", "/*", '"', "'")
_DEP_TABLE_KEYS = ("dependencies", "build-dependencies", "build_dependencies")
_SEMICOLON_ITEM_KWS = frozenset({"use", "type", "static", "const"})
_BLOCK_ITEM_KWS = frozenset(
    {"fn", "mod", "impl", "struct", "enum", "trait", "extern", "macro_rules"}
)
_ALL_ITEM_KWS = _SEMICOLON_ITEM_KWS | _BLOCK_ITEM_KWS

# Minimal TOML fallback regexes.
_TOML_COMMENT_SCAN_RE = re.compile(r'"(?:\\.|[^"\\])*(?:"|$)|\'[^\']*(?:\'|$)|(#)')
_TOML_DOTTED_TOKEN_RE = re.compile(r'"[^"]*"|\'[^\']*\'|\.|[^.\'"]+')
_TOML_ARRAY_ITEM_RE = re.compile(r'"([^"]*)"|\'([^\']*)\'')
_TOML_INLINE_KV_RE = re.compile(
    r'([A-Za-z0-9_-]+)\s*=\s*(?:"([^"]*)"|\'([^\']*)\'|(true|false))'
)

# Single-pass Rust lexical pattern for comments, string literals, and char literals.
_LEX_RE = re.compile(
    r"""
      (?P<line_comment>//[^\n]*)
    | (?P<block_comment>/\*)
    | (?P<raw_str>(?<![A-Za-z0-9_])(?:b|c)?r(?P<hashes>\#*)"[\s\S]*?"(?P=hashes))
    | (?P<str>(?<![A-Za-z0-9_])(?:b|c)?"(?:\\[\s\S]|[^"\\])*")
    | (?P<char>(?<![A-Za-z0-9_])b?'(?:\\(?:x[0-9a-fA-F]{2}|u\{[0-9a-fA-F_]+\}|[^\n])|[^'\\\n])')
    """,
    re.VERBOSE,
)

# Fast regex for files with no `/*` block comments (runs match loop in compiled C).
_LEX_NO_BLOCK_RE = re.compile(
    r"""
      (?P<line_comment>//[^\n]*)
    | (?P<raw_str>(?<![A-Za-z0-9_])(?:b|c)?r(?P<hashes>\#*)"[\s\S]*?"(?P=hashes))
    | (?P<str>(?<![A-Za-z0-9_])(?:b|c)?"(?:\\[\s\S]|[^"\\])*")
    | (?P<char>(?<![A-Za-z0-9_])b?'(?:\\(?:x[0-9a-fA-F]{2}|u\{[0-9a-fA-F_]+\}|[^\n])|[^'\\\n])')
    """,
    re.VERBOSE,
)

_CFG_ATTR_START_RE = re.compile(r"#\s*\[\s*cfg\s*\(")
_STRUCT_TOK_RE = re.compile(
    r"#\s*\[|[{}\[\]();,<>]"
    r"|\b(?:where|fn|mod|impl|struct|enum|trait|type|use|const|static|extern|macro_rules)\b"
)
_IDENT_AT_START_RE = re.compile(r"[A-Za-z_][A-Za-z0-9_]*")
_COLON_OR_EQ_RE = re.compile(r"(?<!:):(?!:)|=")

# Coupling detection patterns (applied to comment/string-masked production Rust code).
_RUSQLITE_RE = re.compile(r"\brusqlite\b")
_UNIX_SOCKET_RE = re.compile(
    r"\b(?:"
    r"std::os::unix::net(?:::(?:UnixStream|UnixListener|UnixDatagram))?"
    r"|tokio::net::Unix(?:Stream|Listener|Datagram)"
    r"|UnixStream|UnixListener|UnixDatagram"
    r")\b"
)
_SANDBOX_CODE_RE = re.compile(
    r"\b(?:"
    r"sandbox_init|sandbox_init_with_parameters|sandbox_check|sandbox_exec"
    r"|(?i:[A-Za-z0-9_]*(?:bwrap|seatbelt)[A-Za-z0-9_]*)"
    r")\b"
)
# Matches unquoted string literals invoking bwrap or sandbox-exec as a binary/command,
# without false-positiving on English prose or diagnostic reason strings.
_SANDBOX_STR_VALUE_RE = re.compile(
    r"(?:^(?:[^\s\"']*/)?(?:bwrap|sandbox-exec)(?:$|\s))"
    r"|(?:(?<=[\s\"'])/(?:[^\s\"']*/)?(?:bwrap|sandbox-exec)(?:$|[\s\"']))"
)
_TRAIT_DECL_RE = re.compile(
    r"\b(?:pub(?:\s*\([^)]*\)\s*|\s+))?(?:unsafe\s+)?(?:auto\s+)?trait\s+([A-Za-z_][A-Za-z0-9_]*)"
)
_ALLOW_CALL_RE = re.compile(r"\b(?:allow|expect)\s*\(")
_DEAD_CODE_WORD_RE = re.compile(r"\bdead_code\b")
_RAW_STR_PREFIX_RE = re.compile(r"^(?:b|c)?r(#*)\"")
_PLAIN_STR_PREFIX_RE = re.compile(r"^(?:b|c)?\"")
_CRATE_NAME_RE = re.compile(r"[A-Za-z0-9_-]+")
_HEX_SHA_RE = re.compile(r"^[0-9a-f]{7,64}$")


@dataclass
class Allowlist:
    crate_loc: dict[str, int] = field(default_factory=dict)
    coupling: dict[str, dict[str, int]] = field(
        default_factory=lambda: {sec: {} for sec in COUPLING_SECTIONS}
    )


@dataclass
class ScanReport:
    crate_locs: dict[str, int]
    coupling_counts: dict[str, dict[str, int]]
    files_scanned: int


@dataclass
class HealthResult:
    errors: list[str]
    warnings: list[str]
    report: ScanReport
    workspace_crates: list[str]
    layer0_checked: list[str]


@dataclass(slots=True)
class _ItemScanState:
    start_pos: int
    seen_let: bool = False
    seen_if: bool = False
    needs_semicolon: bool = False
    in_where: bool = False
    seen_item_kw: bool = False
    paren: int = 0
    bracket: int = 0
    brace: int = 0
    angle: int = 0
    entered_brace: bool = False


# ---------------------------------------------------------------------------
# Minimal Cargo.toml loader (uses stdlib tomllib on 3.11+, pure-stdlib fallback)
# ---------------------------------------------------------------------------


def _strip_toml_comment(line: str) -> str:
    if "#" not in line:
        return line
    for match in _TOML_COMMENT_SCAN_RE.finditer(line):
        if match.group(1) is not None:
            return line[: match.start()]
    return line


def _split_toml_dotted(header: str) -> list[str]:
    parts: list[str] = []
    buf: list[str] = []
    for match in _TOML_DOTTED_TOKEN_RE.finditer(header):
        tok = match.group(0)
        if tok == ".":
            parts.append("".join(buf).strip())
            buf = []
        else:
            buf.append(tok[1:-1] if tok[0] in ('"', "'") else tok)
    parts.append("".join(buf).strip())
    return [part for part in parts if part]


def _ensure_toml_table(root: dict[str, Any], path: list[str]) -> dict[str, Any]:
    target = root
    for part in path:
        nxt = target.get(part)
        if not isinstance(nxt, dict):
            nxt = {}
            target[part] = nxt
        target = nxt
    return target


def _parse_toml_table_header(line: str, root: dict[str, Any]) -> dict[str, Any]:
    if line.startswith("[[") and line.endswith("]]"):
        return {}
    path = _split_toml_dotted(line[1:-1].strip())
    return _ensure_toml_table(root, path)


def _parse_toml_array(inner: str) -> list[str]:
    return [m.group(1) or m.group(2) for m in _TOML_ARRAY_ITEM_RE.finditer(inner)]


def _parse_toml_inline_table(inner: str) -> dict[str, Any]:
    tbl: dict[str, Any] = {}
    for m in _TOML_INLINE_KV_RE.finditer(inner):
        key = m.group(1)
        if m.group(2) is not None:
            tbl[key] = m.group(2)
        elif m.group(3) is not None:
            tbl[key] = m.group(3)
        else:
            tbl[key] = m.group(4) == "true"
    return tbl


def _parse_toml_value(raw: str) -> Any:
    val = raw.strip()
    if len(val) < 2:
        return val
    if val[0] == val[-1] and val[0] in ('"', "'"):
        return val[1:-1]
    if val in ("true", "false"):
        return val == "true"
    if (val[0], val[-1]) == ("[", "]"):
        return _parse_toml_array(val[1:-1])
    if (val[0], val[-1]) == ("{", "}"):
        return _parse_toml_inline_table(val[1:-1])
    return val


def _is_unclosed_toml_collection(rhs: str) -> bool:
    if rhs.startswith("["):
        return rhs.count("[") > rhs.count("]")
    if rhs.startswith("{"):
        return rhs.count("{") > rhs.count("}")
    return False


def _parse_toml_kv(
    line: str, lines: list[str], idx: int, current: dict[str, Any]
) -> int:
    lhs, rhs = line.split("=", 1)
    rhs = rhs.strip()
    while _is_unclosed_toml_collection(rhs) and idx < len(lines):
        rhs += " " + _strip_toml_comment(lines[idx]).strip()
        idx += 1
    keys = _split_toml_dotted(lhs.strip())
    if keys:
        target = _ensure_toml_table(current, keys[:-1])
        target[keys[-1]] = _parse_toml_value(rhs)
    return idx


def _parse_toml_fallback(text: str) -> dict[str, Any]:
    """Minimal pure-stdlib TOML parser covering Cargo.toml tables, arrays, and strings."""
    root: dict[str, Any] = {}
    current: dict[str, Any] = root
    lines = text.splitlines()
    i = 0
    while i < len(lines):
        line = _strip_toml_comment(lines[i]).strip()
        i += 1
        if not line:
            continue
        if line.startswith("[") and line.endswith("]"):
            current = _parse_toml_table_header(line, root)
        elif "=" in line:
            i = _parse_toml_kv(line, lines, i, current)
    return root


def load_toml(path: Path) -> dict[str, Any]:
    if tomllib is not None:
        with path.open("rb") as f:
            return tomllib.load(f)
    return _parse_toml_fallback(path.read_text(encoding="utf-8"))


# ---------------------------------------------------------------------------
# Rust lexical masking, #[cfg(test)] stripping, and non-test LOC counting
# ---------------------------------------------------------------------------


def is_test_source_path(rel_to_crate_posix: str) -> bool:
    """Return True if a crate-relative `.rs` path belongs to dedicated test code."""
    stem_path = rel_to_crate_posix[:-3] if rel_to_crate_posix.endswith(".rs") else rel_to_crate_posix
    for part in stem_path.split("/"):
        if (
            part in TEST_EXACT_NAMES
            or part.startswith(TEST_PREFIXES)
            or part.endswith(TEST_SUFFIXES)
        ):
            return True
    return False


def _unquote_rust_string(raw: str) -> str:
    m = _RAW_STR_PREFIX_RE.match(raw)
    if m:
        hashes = len(m.group(1))
        return raw[m.end() : len(raw) - 1 - hashes]
    m2 = _PLAIN_STR_PREFIX_RE.match(raw)
    if m2:
        return raw[m2.end() : -1]
    return raw


def _blank_preserving_newlines(seg: str) -> str:
    if "\n" not in seg:
        return " " * len(seg)
    return "\n".join(" " * len(part) for part in seg.split("\n"))


def _skip_block_comment(src: str, pos: int) -> int:
    """Given `pos` right after `/*`, return index right after its matching nested `*/`."""
    depth = 1
    n = len(src)
    while pos < n and depth > 0:
        nxt_open = src.find("/*", pos)
        nxt_close = src.find("*/", pos)
        if nxt_close == -1:
            return n
        if nxt_open != -1 and nxt_open < nxt_close:
            depth += 1
            pos = nxt_open + 2
        else:
            depth -= 1
            pos = nxt_close + 2
    return pos


def _mask_without_block_comments(src: str) -> tuple[str, list[tuple[int, int, str]]]:
    strings: list[tuple[int, int, str]] = []

    def _repl(m: re.Match[str]) -> str:
        if m.lastgroup in ("str", "raw_str"):
            strings.append((m.start(), m.end(), m.group(0)))
        return _blank_preserving_newlines(m.group(0))

    return _LEX_NO_BLOCK_RE.sub(_repl, src), strings


def mask_comments_and_strings(src: str) -> tuple[str, list[tuple[int, int, str]]]:
    """Replace Rust comments, strings, and char literals with spaces preserving newlines.

    Returns `(code_only, string_spans)` where `string_spans` holds `(start, end, raw_literal)`
    for every non-comment string literal.
    """
    if not any(marker in src for marker in _LEX_MARKERS):
        return src, []
    if "/*" not in src:
        return _mask_without_block_comments(src)

    strings: list[tuple[int, int, str]] = []
    out: list[str] = []
    pos = 0
    last = 0
    n = len(src)
    while pos < n:
        m = _LEX_RE.search(src, pos)
        if not m:
            break
        start, end = m.start(), m.end()
        if m.lastgroup == "block_comment":
            end = _skip_block_comment(src, end)
        elif m.lastgroup in ("str", "raw_str"):
            strings.append((start, end, src[start:end]))
        out.append(src[last:start])
        out.append(_blank_preserving_newlines(src[start:end]))
        last = pos = end

    out.append(src[last:])
    return "".join(out), strings


def _skip_balanced(code: str, pos: int, open_ch: str, close_ch: str) -> int:
    """Given `pos` right after `open_ch`, return index right after its matching `close_ch`."""
    depth = 1
    n = len(code)
    while pos < n and depth > 0:
        nxt_open = code.find(open_ch, pos)
        nxt_close = code.find(close_ch, pos)
        if nxt_close == -1:
            return n
        if nxt_open != -1 and nxt_open < nxt_close:
            depth += 1
            pos = nxt_open + 1
        else:
            depth -= 1
            pos = nxt_close + 1
    return pos


def _split_top_level_args(text: str) -> list[str]:
    parts: list[str] = []
    buf: list[str] = []
    depth = 0
    for ch in text:
        if ch == "(":
            depth += 1
            buf.append(ch)
        elif ch == ")":
            depth = max(0, depth - 1)
            buf.append(ch)
        elif ch == "," and depth == 0:
            parts.append("".join(buf).strip())
            buf = []
        else:
            buf.append(ch)
    tail = "".join(buf).strip()
    if tail:
        parts.append(tail)
    return parts


def _is_test_only_cfg_predicate(pred: str) -> bool:
    """Return True iff a `cfg(...)` predicate compiles only under `#[cfg(test)]`."""
    compact = pred.strip()
    if compact == "test":
        return True
    if compact.startswith("all") and compact.endswith(")"):
        rest = compact[3:].lstrip()
        if rest.startswith("(") and _skip_balanced(rest, 1, "(", ")") == len(rest):
            inner = rest[1:-1]
            return any(arg == "test" for arg in _split_top_level_args(inner))
    return False


def _skip_ws_forward(code: str, pos: int) -> int:
    n = len(code)
    while pos < n and code[pos].isspace():
        pos += 1
    return pos


def _skip_ws_backward(code: str, idx: int, min_start: int) -> int:
    while idx > min_start and code[idx - 1].isspace():
        idx -= 1
    return idx


def _find_matching_open_bracket(code: str, close_idx: int, min_start: int) -> int | None:
    depth = 1
    j = close_idx - 1
    while j >= min_start and depth > 0:
        ch = code[j]
        if ch == "]":
            depth += 1
        elif ch == "[":
            depth -= 1
        j -= 1
    return (j + 1) if depth == 0 else None


def _prev_outer_attr_span(
    code: str, idx: int, min_start: int = 0
) -> tuple[int, int] | None:
    end = _skip_ws_backward(code, idx, min_start)
    if end <= min_start or code[end - 1] != "]":
        return None
    open_bracket = _find_matching_open_bracket(code, end - 1, min_start)
    if open_bracket is None:
        return None
    before_bracket = _skip_ws_backward(code, open_bracket, min_start)
    if before_bracket <= min_start or code[before_bracket - 1] != "#":
        return None
    return before_bracket - 1, end


def _walk_back_outer_attrs(code: str, item_start: int, min_start: int) -> int:
    while True:
        span = _prev_outer_attr_span(code, item_start, min_start)
        if span is None:
            return item_start
        item_start = span[0]


def _skip_chained_outer_attrs(code: str, pos: int) -> int:
    n = len(code)
    while True:
        pos = _skip_ws_forward(code, pos)
        if pos >= n or code[pos] != "#":
            return pos
        bracket = _skip_ws_forward(code, pos + 1)
        if bracket >= n or code[bracket] != "[":
            return pos
        pos = _skip_balanced(code, bracket + 1, "[", "]")


def _match_cfg_test_attr(code: str, m: re.Match[str]) -> tuple[bool, int]:
    paren_end = _skip_balanced(code, m.end(), "(", ")")
    pred = code[m.end() : paren_end - 1]
    bracket_pos = code.find("[", m.start())
    attr_end = _skip_balanced(code, bracket_pos + 1, "[", "]")
    is_only_cfg = code[paren_end : attr_end - 1].strip() == ""
    return is_only_cfg and _is_test_only_cfg_predicate(pred), attr_end


def _at_top_level(state: _ItemScanState) -> bool:
    return state.paren == 0 and state.bracket == 0 and state.brace == 0


def _init_item_scan_state(code: str, pos: int) -> _ItemScanState:
    state = _ItemScanState(start_pos=pos)
    wm = _IDENT_AT_START_RE.match(code, pos)
    if not wm:
        return state
    first_kw = wm.group(0)
    if first_kw == "let":
        state.seen_let = True
        state.needs_semicolon = True
    elif first_kw == "if":
        state.seen_if = True
    elif first_kw in _SEMICOLON_ITEM_KWS:
        state.seen_item_kw = True
        state.needs_semicolon = True
    elif first_kw in _BLOCK_ITEM_KWS:
        state.seen_item_kw = True
    return state


def _update_paren_or_bracket(tok: str, state: _ItemScanState) -> bool:
    if tok == "(":
        state.paren += 1
    elif tok == ")":
        state.paren = max(0, state.paren - 1)
    elif tok == "[":
        state.bracket += 1
    elif tok == "]":
        state.bracket = max(0, state.bracket - 1)
    else:
        return False
    return True


def _handle_open_angle(code: str, end: int, state: _ItemScanState) -> None:
    if state.seen_let or state.seen_if:
        return
    if code[end : end + 1] not in ("=", "<"):
        state.angle += 1


def _handle_close_angle(code: str, start: int, state: _ItemScanState) -> None:
    if state.angle == 0:
        return
    if start == 0 or code[start - 1] not in ("-", "="):
        state.angle -= 1


def _handle_angle(code: str, tm: re.Match[str], state: _ItemScanState) -> None:
    if not _at_top_level(state):
        return
    if tm.group(0) == "<":
        _handle_open_angle(code, tm.end(), state)
    else:
        _handle_close_angle(code, tm.start(), state)


def _handle_open_brace(state: _ItemScanState) -> None:
    state.brace += 1
    state.entered_brace = True
    if state.paren == 0 and state.bracket == 0:
        state.in_where = False
        state.angle = 0


def _match_following_else(code: str, cur: int) -> int | None:
    q = _skip_ws_forward(code, cur)
    if code[q : q + 4] != "else":
        return None
    after = code[q + 4 : q + 5]
    if after and (after.isalnum() or after == "_"):
        return None
    return q + 4


def _handle_close_brace(
    code: str, tm: re.Match[str], state: _ItemScanState
) -> tuple[int, bool]:
    if state.brace == 0:
        return tm.start(), True
    state.brace -= 1
    cur = tm.end()
    if not (_at_top_level(state) and state.entered_brace):
        return cur, False
    if state.needs_semicolon:
        return cur, False
    if state.seen_if:
        else_end = _match_following_else(code, cur)
        if else_end is not None:
            state.entered_brace = False
            return else_end, False
    return cur, True


def _handle_delimiter(
    code: str, tm: re.Match[str], state: _ItemScanState
) -> tuple[int, bool] | None:
    tok = tm.group(0)
    if tok.startswith("#"):
        return _skip_balanced(code, tm.end(), "[", "]"), False
    if _update_paren_or_bracket(tok, state):
        return tm.end(), False
    if tok == "{":
        _handle_open_brace(state)
        return tm.end(), False
    if tok == "}":
        return _handle_close_brace(code, tm, state)
    if tok in ("<", ">"):
        _handle_angle(code, tm, state)
        return tm.end(), False
    return None


def _is_field_terminator_comma(state: _ItemScanState) -> bool:
    return (
        state.angle == 0
        and not state.seen_item_kw
        and not state.in_where
        and not state.seen_let
        and not state.needs_semicolon
        and not state.seen_if
    )


def _is_static_lifetime(code: str, tm: re.Match[str]) -> bool:
    return tm.group(0) == "static" and tm.start() > 0 and code[tm.start() - 1] == "'"


def _clears_semicolon_requirement(
    code: str, tm: re.Match[str], state: _ItemScanState
) -> bool:
    if not state.needs_semicolon or state.entered_brace or state.seen_let:
        return False
    if tm.group(0) not in ("fn", "trait"):
        return False
    return _COLON_OR_EQ_RE.search(code, state.start_pos, tm.start()) is None


def _apply_item_keyword(
    code: str, tm: re.Match[str], state: _ItemScanState
) -> None:
    if _is_static_lifetime(code, tm):
        return
    tok = tm.group(0)
    if (
        tok in _SEMICOLON_ITEM_KWS
        and not state.seen_item_kw
        and not state.entered_brace
        and not state.seen_if
        and not state.seen_let
    ):
        state.needs_semicolon = True
    if _clears_semicolon_requirement(code, tm, state):
        state.needs_semicolon = False
    state.seen_item_kw = True


def _update_item_keywords(
    code: str, tm: re.Match[str], state: _ItemScanState
) -> bool:
    if not _at_top_level(state):
        return False
    tok = tm.group(0)
    if tok == ";":
        return True
    if tok == ",":
        return _is_field_terminator_comma(state)
    if tok == "where":
        state.in_where = True
        return False
    if state.angle == 0 and tok in _ALL_ITEM_KWS:
        _apply_item_keyword(code, tm, state)
    return False


def _scan_item_tokens(code: str, start_pos: int, state: _ItemScanState) -> int:
    n = len(code)
    cur = start_pos
    while cur < n:
        tm = _STRUCT_TOK_RE.search(code, cur)
        if not tm:
            return n
        delim_res = _handle_delimiter(code, tm, state)
        if delim_res is not None and delim_res[1]:
            return delim_res[0]
        if delim_res is not None:
            cur = delim_res[0]
        elif _update_item_keywords(code, tm, state):
            return tm.end()
        else:
            cur = tm.end()
    return cur


def _find_cfg_test_item_end(code: str, attr_end: int) -> int:
    item_body_start = _skip_chained_outer_attrs(code, attr_end)
    state = _init_item_scan_state(code, item_body_start)
    return _scan_item_tokens(code, item_body_start, state)


def find_cfg_test_ranges(code: str) -> list[tuple[int, int]]:
    """Find character `(start, end)` ranges of `#[cfg(test)]` / `#[cfg(all(..., test, ...))]` items."""
    if "cfg" not in code:
        return []
    n = len(code)
    ranges: list[tuple[int, int]] = []
    pos = 0
    while pos < n:
        m = _CFG_ATTR_START_RE.search(code, pos)
        if not m:
            break
        is_test_only, attr_end = _match_cfg_test_attr(code, m)
        if not is_test_only:
            pos = attr_end
            continue
        min_start = ranges[-1][1] if ranges else 0
        item_start = _walk_back_outer_attrs(code, m.start(), min_start)
        item_end = _find_cfg_test_item_end(code, attr_end)
        ranges.append((item_start, item_end))
        pos = item_end
    return ranges


def _advance_line_mask(ranges: list[tuple[int, int]], ri: int, line_start: int) -> int:
    nr = len(ranges)
    while ri < nr and ranges[ri][1] <= line_start:
        ri += 1
    return ri


def _is_code_line(
    src: str,
    line_start: int,
    line_end: int,
    ranges: list[tuple[int, int]],
    ri: int,
) -> bool:
    nr = len(ranges)
    if ri >= nr or ranges[ri][0] >= line_end:
        return True
    cur = line_start
    j = ri
    while j < nr and ranges[j][0] < line_end:
        rs, re_ = ranges[j]
        if cur < rs and src[cur : min(rs, line_end)].strip():
            return True
        cur = max(cur, re_)
        j += 1
    return cur < line_end and bool(src[cur:line_end].strip())


def count_non_test_lines(src: str, ranges: list[tuple[int, int]]) -> int:
    """Count lines in `src` excluding lines wholly covered by `#[cfg(test)]` `ranges`."""
    if not src:
        return 0
    if not ranges:
        return src.count("\n") + (0 if src.endswith("\n") else 1)
    total = 0
    ri = 0
    n = len(src)
    ls = 0
    while ls < n:
        nl = src.find("\n", ls)
        le = nl if nl != -1 else n
        ri = _advance_line_mask(ranges, ri, ls)
        if _is_code_line(src, ls, le, ranges, ri):
            total += 1
        if nl == -1:
            break
        ls = nl + 1
    return total


def blank_ranges(code: str, ranges: list[tuple[int, int]]) -> str:
    if not ranges:
        return code
    out: list[str] = []
    last = 0
    for s, e in ranges:
        out.append(code[last:s])
        out.append(_blank_preserving_newlines(code[s:e]))
        last = e
    out.append(code[last:])
    return "".join(out)


def _offset_in_ranges(offset: int, ranges: list[tuple[int, int]]) -> bool:
    for s, e in ranges:
        if s <= offset < e:
            return True
        if s > offset:
            return False
    return False


# ---------------------------------------------------------------------------
# Coupling & dead_code-on-trait detectors
# ---------------------------------------------------------------------------


def _attr_has_dead_code(attr_text: str) -> bool:
    for m in _ALLOW_CALL_RE.finditer(attr_text):
        end = _skip_balanced(attr_text, m.end(), "(", ")")
        inner = attr_text[m.end() : end - 1]
        if _DEAD_CODE_WORD_RE.search(inner):
            return True
    return False


def _scan_preceding_attrs(prod_code: str, start_idx: int) -> list[str]:
    attrs: list[str] = []
    idx = start_idx
    while True:
        span = _prev_outer_attr_span(prod_code, idx)
        if span is None:
            return attrs
        attrs.append(prod_code[span[0] : span[1]])
        idx = span[0]


def find_dead_code_traits(prod_code: str) -> list[str]:
    """Return names of traits directly preceded by an outer `allow|expect(...dead_code...)` attribute."""
    if "dead_code" not in prod_code or "trait" not in prod_code:
        return []
    hits: list[str] = []
    for tm in _TRAIT_DECL_RE.finditer(prod_code):
        attrs = _scan_preceding_attrs(prod_code, tm.start())
        if any(_attr_has_dead_code(attr) for attr in attrs):
            hits.append(tm.group(1))
    return hits


def _matches_adapter(rel_posix: str, adapters: tuple[str, ...]) -> bool:
    for entry in adapters:
        if entry.endswith("/"):
            if rel_posix.startswith(entry):
                return True
        elif rel_posix == entry:
            return True
    return False


def _count_rusqlite(rel_posix: str, prod_code: str) -> int:
    if _matches_adapter(rel_posix, RUSQLITE_ADAPTER_PREFIXES) or "rusqlite" not in prod_code:
        return 0
    return len(_RUSQLITE_RE.findall(prod_code))


def _count_unix_socket(rel_posix: str, prod_code: str) -> int:
    if _matches_adapter(rel_posix, UNIX_SOCKET_ADAPTER_PREFIXES):
        return 0
    if "Unix" not in prod_code and "unix::net" not in prod_code:
        return 0
    return len(_UNIX_SOCKET_RE.findall(prod_code))


def _count_sandbox_strings(
    text: str,
    string_spans: list[tuple[int, int, str]],
    ranges: list[tuple[int, int]],
) -> int:
    if "bwrap" not in text and "sandbox-exec" not in text:
        return 0
    count = 0
    for s_start, _s_end, raw_lit in string_spans:
        if "bwrap" not in raw_lit and "sandbox-exec" not in raw_lit:
            continue
        if _offset_in_ranges(s_start, ranges):
            continue
        if _SANDBOX_STR_VALUE_RE.search(_unquote_rust_string(raw_lit).strip()):
            count += 1
    return count


def _count_sandbox(
    rel_posix: str,
    text: str,
    prod_code: str,
    string_spans: list[tuple[int, int, str]],
    ranges: list[tuple[int, int]],
) -> int:
    if _matches_adapter(rel_posix, SANDBOX_ADAPTER_PREFIXES):
        return 0
    code_hits = 0
    lower_prod = prod_code.lower()
    if "sandbox" in prod_code or "bwrap" in lower_prod or "seatbelt" in lower_prod:
        code_hits = len(_SANDBOX_CODE_RE.findall(prod_code))
    return code_hits + _count_sandbox_strings(text, string_spans, ranges)


def scan_production_file(
    rel_posix: str, text: str
) -> tuple[int, int, int, int, int]:
    """Return `(non_test_loc, rusqlite, unix_socket, sandbox, dead_code_trait)` for one file."""
    has_fast_trigger = any(trigger in text for trigger in FAST_PATH_TRIGGERS)
    if not has_fast_trigger:
        lower_text = text.lower()
        has_fast_trigger = "bwrap" in lower_text or "seatbelt" in lower_text
    if not has_fast_trigger:
        loc = text.count("\n") + (0 if not text or text.endswith("\n") else 1)
        return loc, 0, 0, 0, 0

    code, string_spans = mask_comments_and_strings(text)
    ranges = find_cfg_test_ranges(code)
    loc = count_non_test_lines(text, ranges)
    prod_code = blank_ranges(code, ranges)

    return (
        loc,
        _count_rusqlite(rel_posix, prod_code),
        _count_unix_socket(rel_posix, prod_code),
        _count_sandbox(rel_posix, text, prod_code, string_spans, ranges),
        len(find_dead_code_traits(prod_code)),
    )


# ---------------------------------------------------------------------------
# Workspace Cargo.toml discovery, DAG cycle check, and Layer-0 guard
# ---------------------------------------------------------------------------


def _extract_workspace_dep_renames(root_manifest: dict[str, Any]) -> dict[str, str]:
    """Return `{alias: package_name}` for renamed entries in `[workspace.dependencies]`."""
    renames: dict[str, str] = {}
    ws_deps = (root_manifest.get("workspace") or {}).get("dependencies")
    if isinstance(ws_deps, dict):
        for alias, spec in ws_deps.items():
            if isinstance(spec, dict) and isinstance(spec.get("package"), str):
                renames[alias] = spec["package"]
    return renames


def _expand_member_pattern(repo_root: Path, pattern: str) -> list[Path]:
    if any(ch in pattern for ch in "*?["):
        return sorted(p for p in repo_root.glob(pattern) if (p / "Cargo.toml").is_file())
    return [repo_root / pattern]


def _resolve_workspace_members(repo_root: Path, raw_members: list[str]) -> list[Path]:
    candidates = [
        cand
        for pattern in raw_members
        for cand in _expand_member_pattern(repo_root, pattern)
    ]
    member_dirs: list[Path] = []
    seen_dirs: set[Path] = set()
    for cand in candidates:
        resolved = cand.resolve()
        if resolved not in seen_dirs:
            seen_dirs.add(resolved)
            member_dirs.append(cand)
    return member_dirs


def _load_member_manifest(member_dir: Path) -> tuple[str, Path, dict[str, Any]]:
    manifest_path = member_dir / "Cargo.toml"
    if not manifest_path.is_file():
        raise ValueError(f"workspace member missing Cargo.toml: {member_dir}")
    data = load_toml(manifest_path)
    pkg_name = (data.get("package") or {}).get("name")
    if not isinstance(pkg_name, str) or not pkg_name:
        raise ValueError(f"{manifest_path}: missing [package].name")
    return pkg_name, member_dir, data


def _load_workspace(
    repo_root: Path,
) -> tuple[list[tuple[str, Path, dict[str, Any]]], dict[str, str]]:
    """Load root `Cargo.toml` and return `(sorted_members, workspace_dep_renames)`."""
    root_manifest_path = repo_root / "Cargo.toml"
    if not root_manifest_path.is_file():
        raise ValueError(f"missing root Cargo.toml at {root_manifest_path}")
    root_manifest = load_toml(root_manifest_path)
    raw_members = (root_manifest.get("workspace") or {}).get("members") or []
    if not raw_members:
        raise ValueError("root Cargo.toml declares no [workspace].members")
    workspace_dep_renames = _extract_workspace_dep_renames(root_manifest)
    member_dirs = _resolve_workspace_members(repo_root, raw_members)
    members = [_load_member_manifest(member_dir) for member_dir in member_dirs]
    members.sort(key=lambda item: item[0])
    return members, workspace_dep_renames


def discover_workspace_members(repo_root: Path) -> list[tuple[str, Path, dict[str, Any]]]:
    """Return sorted `[(package_name, member_dir, manifest_dict), ...]` from root `Cargo.toml`."""
    members, _ = _load_workspace(repo_root)
    return members


def _extract_workspace_deps(
    dep_table: Any,
    workspace_pkgs: set[str],
    workspace_dep_renames: dict[str, str] | None = None,
) -> set[str]:
    deps: set[str] = set()
    if not isinstance(dep_table, dict):
        return deps
    for key, val in dep_table.items():
        pkg_name = key
        if isinstance(val, dict):
            if isinstance(val.get("package"), str):
                pkg_name = val["package"]
            if (
                val.get("workspace") is True
                and workspace_dep_renames
                and pkg_name in workspace_dep_renames
            ):
                pkg_name = workspace_dep_renames[pkg_name]
        if pkg_name in workspace_pkgs:
            deps.add(pkg_name)
    return deps


def _extract_section_deps(
    table: dict[str, Any],
    workspace_pkgs: set[str],
    workspace_dep_renames: dict[str, str] | None,
) -> set[str]:
    deps: set[str] = set()
    for key in _DEP_TABLE_KEYS:
        deps |= _extract_workspace_deps(
            table.get(key), workspace_pkgs, workspace_dep_renames
        )
    return deps


def _extract_target_deps(
    targets: Any,
    workspace_pkgs: set[str],
    workspace_dep_renames: dict[str, str] | None,
) -> set[str]:
    if not isinstance(targets, dict):
        return set()
    deps: set[str] = set()
    for tdata in targets.values():
        if isinstance(tdata, dict):
            deps |= _extract_section_deps(tdata, workspace_pkgs, workspace_dep_renames)
    return deps


def build_normal_dep_graph(
    members: list[tuple[str, Path, dict[str, Any]]],
    workspace_dep_renames: dict[str, str] | None = None,
) -> dict[str, list[str]]:
    """Build workspace normal + build dependency adjacency list (excluding dev-dependencies)."""
    workspace_pkgs = {name for name, _, _ in members}
    graph: dict[str, list[str]] = {}
    for name, _member_dir, data in members:
        deps = _extract_section_deps(data, workspace_pkgs, workspace_dep_renames)
        deps |= _extract_target_deps(data.get("target"), workspace_pkgs, workspace_dep_renames)
        graph[name] = sorted(deps)
    return graph


def find_dependency_cycles(graph: dict[str, list[str]]) -> list[list[str]]:
    """Return deterministic cycle paths (e.g. `['a', 'b', 'a']`) in `graph` via DFS."""
    WHITE, GRAY, BLACK = 0, 1, 2
    color: dict[str, int] = {node: WHITE for node in graph}
    stack: list[str] = []
    stack_index: dict[str, int] = {}
    cycles: list[list[str]] = []
    seen_canonical: set[tuple[str, ...]] = set()

    def _dfs(node: str) -> None:
        color[node] = GRAY
        stack_index[node] = len(stack)
        stack.append(node)
        for nxt in graph.get(node, ()):
            state = color.get(nxt, WHITE)
            if state == GRAY:
                cycle_nodes = stack[stack_index[nxt] :]
                # Canonicalize cycle rotation for deduplication.
                min_i = min(range(len(cycle_nodes)), key=cycle_nodes.__getitem__)
                canonical = tuple(cycle_nodes[min_i:] + cycle_nodes[:min_i])
                if canonical not in seen_canonical:
                    seen_canonical.add(canonical)
                    cycles.append([*canonical, canonical[0]])
            elif state == WHITE:
                _dfs(nxt)
        stack.pop()
        del stack_index[node]
        color[node] = BLACK

    for node in sorted(graph):
        if color[node] == WHITE:
            _dfs(node)
    return cycles


def check_cargo_dag_and_layer0(
    members: list[tuple[str, Path, dict[str, Any]]],
    workspace_dep_renames: dict[str, str] | None = None,
) -> tuple[list[str], list[str]]:
    """Verify acyclic workspace DAG ([dependencies]/[build-dependencies]) and Layer-0 stability."""
    errors: list[str] = []
    graph = build_normal_dep_graph(members, workspace_dep_renames=workspace_dep_renames)

    for cycle in find_dependency_cycles(graph):
        errors.append(
            f"Cargo DAG cycle detected in [dependencies]/[build-dependencies]: {' -> '.join(cycle)}"
        )

    workspace_pkgs = set(graph.keys())
    layer0_present = sorted(LAYER_0_ALLOWED_WORKSPACE_DEPS & workspace_pkgs)
    for crate in layer0_present:
        for dep in graph[crate]:
            if dep not in LAYER_0_ALLOWED_WORKSPACE_DEPS:
                errors.append(
                    f"Layer-0 stability violation: `{crate}` has a [dependencies]/[build-dependencies] edge "
                    f"to non-Layer-0 crate `{dep}` (Layer-0 crates may only depend on std, "
                    f"external crates, or other Layer-0 crates)."
                )
    return errors, layer0_present


# ---------------------------------------------------------------------------
# Workspace source walk
# ---------------------------------------------------------------------------


def _is_skipped_dir(entry: os.DirEntry[str]) -> bool:
    name = entry.name
    if name.startswith(".") or name in SKIPPED_DIRS:
        return True
    dir_path = Path(entry.path)
    return (dir_path / "CACHEDIR.TAG").exists() or (dir_path / ".git").exists()


def _read_dir_entries(current: Path) -> list[os.DirEntry[str]]:
    try:
        return list(os.scandir(current))
    except OSError as exc:
        raise ValueError(f"cannot read directory {current}: {exc}") from exc


def _iter_crate_rs_files(crate_dir: Path) -> list[Path]:
    found: list[Path] = []
    stack = [crate_dir]
    while stack:
        for entry in _read_dir_entries(stack.pop()):
            if entry.is_dir(follow_symlinks=False) and not _is_skipped_dir(entry):
                stack.append(Path(entry.path))
            elif entry.name.endswith(".rs") and entry.is_file(follow_symlinks=True):
                found.append(Path(entry.path))
    found.sort()
    return found


def scan_workspace(
    repo_root: Path, members: list[tuple[str, Path, dict[str, Any]]]
) -> ScanReport:
    crate_locs: dict[str, int] = {}
    coupling: dict[str, dict[str, int]] = {sec: {} for sec in COUPLING_SECTIONS}
    files_scanned = 0

    for pkg_name, member_dir, _data in members:
        loc_total = 0
        for rs_path in _iter_crate_rs_files(member_dir):
            rel_to_crate = rs_path.relative_to(member_dir).as_posix()
            if is_test_source_path(rel_to_crate):
                continue
            rel_posix = rs_path.relative_to(repo_root).as_posix()
            files_scanned += 1
            text = rs_path.read_text(encoding="utf-8", errors="replace")
            loc, rusq, unix, sbox, dct = scan_production_file(rel_posix, text)
            loc_total += loc
            if rusq:
                coupling[SECTION_RUSQLITE][rel_posix] = rusq
            if unix:
                coupling[SECTION_UNIX_SOCKET][rel_posix] = unix
            if sbox:
                coupling[SECTION_SANDBOX][rel_posix] = sbox
            if dct:
                coupling[SECTION_DEAD_CODE_TRAIT][rel_posix] = dct
        crate_locs[pkg_name] = loc_total

    return ScanReport(
        crate_locs=crate_locs,
        coupling_counts={sec: dict(sorted(vals.items())) for sec, vals in coupling.items()},
        files_scanned=files_scanned,
    )


# ---------------------------------------------------------------------------
# Allowlist parsing, validation, formatting, and shrink-only base comparison
# ---------------------------------------------------------------------------


def _is_section_header_line(raw_line: str) -> bool:
    return raw_line.startswith("[") and raw_line.endswith("]")


def _parse_allowlist_section_header(
    raw_line: str, seen_sections: set[str], source_name: str, lineno: int
) -> str:
    sec = raw_line[1:-1]
    if sec not in ALL_SECTIONS:
        raise ValueError(f"{source_name}:{lineno}: unknown section [{sec}]")
    if sec in seen_sections:
        raise ValueError(f"{source_name}:{lineno}: duplicate section [{sec}]")
    seen_sections.add(sec)
    return sec


def _is_plain_rs_path(key: str) -> bool:
    if not key.endswith(".rs") or "\\" in key or any(ch.isspace() for ch in key):
        return False
    return all(part not in ("", ".", "..") for part in key.split("/"))


def _store_allowlist_entry(
    parsed: Allowlist,
    section: str,
    key: str,
    count: int,
    source_name: str,
    lineno: int,
) -> None:
    if section == SECTION_CRATE_LOC:
        if not _CRATE_NAME_RE.fullmatch(key):
            raise ValueError(f"{source_name}:{lineno}: invalid crate name {key!r}")
        parsed.crate_loc[key] = count
        return
    if not _is_plain_rs_path(key):
        raise ValueError(
            f"{source_name}:{lineno}: expected plain repo-relative `.rs` path, got {key!r}"
        )
    parsed.coupling[section][key] = count


def _parse_allowlist_entry(
    raw_line: str,
    section: str | None,
    prev_key: str | None,
    parsed: Allowlist,
    source_name: str,
    lineno: int,
) -> str:
    if section is None:
        raise ValueError(f"{source_name}:{lineno}: entry appears before any [section] header")
    parts = raw_line.split(" ")
    if len(parts) != 2:
        raise ValueError(
            f"{source_name}:{lineno}: expected `<key> <positive-int>` with a single space"
        )
    key, count_str = parts
    if not count_str.isdigit() or count_str.startswith("0"):
        raise ValueError(
            f"{source_name}:{lineno}: count must be a positive decimal integer without leading zero, got {count_str!r}"
        )
    if prev_key is not None and prev_key >= key:
        raise ValueError(
            f"{source_name}:{lineno}: entries in [{section}] must be strictly sorted and unique"
        )
    _store_allowlist_entry(parsed, section, key, int(count_str), source_name, lineno)
    return key


def _validate_required_sections(seen_sections: set[str], source_name: str) -> None:
    missing = [sec for sec in ALL_SECTIONS if sec not in seen_sections]
    if missing:
        raise ValueError(f"{source_name}: missing required section(s): {', '.join(missing)}")


def parse_allowlist(text: str, source_name: str = ALLOWLIST_REL) -> Allowlist:
    if not text.startswith("#"):
        raise ValueError(
            f"{source_name}:1: allowlist must open with a `#` comment explaining its shrink-only policy"
        )
    parsed = Allowlist()
    current_section: str | None = None
    seen_sections: set[str] = set()
    prev_key: str | None = None

    for lineno, raw_line in enumerate(text.splitlines(), start=1):
        if raw_line == "" or raw_line.startswith("#"):
            continue
        if _is_section_header_line(raw_line):
            current_section = _parse_allowlist_section_header(
                raw_line, seen_sections, source_name, lineno
            )
            prev_key = None
        else:
            prev_key = _parse_allowlist_entry(
                raw_line, current_section, prev_key, parsed, source_name, lineno
            )

    _validate_required_sections(seen_sections, source_name)
    return parsed


def _check_single_crate_loc(
    crate: str,
    loc: int,
    allowlist_loc: dict[str, int],
    allowlist_label: str,
    errors: list[str],
    warnings: list[str],
) -> None:
    if crate in allowlist_loc:
        ceiling = allowlist_loc[crate]
        if loc > ceiling:
            errors.append(
                f"[crate_loc] `{crate}` has {loc:,} non-test LOC, exceeding its shrink-only "
                f"baseline ceiling of {ceiling:,} in {allowlist_label}. Extract new domains "
                f"into a cohesive crate ({RULES_DOC})."
            )
        return
    if loc > CRATE_FAIL_LOC:
        errors.append(
            f"[crate_loc] unlisted crate `{crate}` has {loc:,} non-test LOC, exceeding "
            f"the {CRATE_FAIL_LOC:,}-LOC hard cap. Extract domains into a cohesive crate "
            f"({RULES_DOC}); new crates may not be added to {allowlist_label}."
        )
    elif loc >= CRATE_WARN_LOC:
        warnings.append(
            f"[crate_loc] WARNING: crate `{crate}` has {loc:,} non-test LOC "
            f"(>= {CRATE_WARN_LOC:,} warning threshold; hard cap is {CRATE_FAIL_LOC:,})."
        )


def _check_crate_loc_limits(
    crate_locs: dict[str, int], allowlist_loc: dict[str, int], allowlist_label: str
) -> tuple[list[str], list[str]]:
    errors: list[str] = []
    warnings: list[str] = []
    for crate, loc in sorted(crate_locs.items()):
        _check_single_crate_loc(crate, loc, allowlist_loc, allowlist_label, errors, warnings)
    for crate in sorted(allowlist_loc):
        if crate not in crate_locs:
            errors.append(
                f"[crate_loc] `{crate}` in {allowlist_label} is no longer a workspace crate; "
                f"delete its entry."
            )
    return errors, warnings


def _check_coupling_section(
    sec: str,
    actual_map: dict[str, int],
    listed_map: dict[str, int],
    allowlist_label: str,
) -> list[str]:
    errors: list[str] = []
    for path, actual in sorted(actual_map.items()):
        listed = listed_map.get(path)
        if listed is None:
            errors.append(
                f"[{sec}] {path}: {actual} occurrence(s) outside designated adapter "
                f"(unlisted in {allowlist_label}). Move behind an adapter/port ({RULES_DOC})."
            )
        elif actual > listed:
            errors.append(
                f"[{sec}] {path}: {actual} occurrence(s), exceeding allowlist baseline "
                f"{listed} in {allowlist_label}. Coupling allowlist only shrinks."
            )
        elif actual < listed:
            errors.append(
                f"[{sec}] {path}: {actual} occurrence(s), below allowlist entry {listed}. "
                f"Lower its entry in {allowlist_label} to {actual} so the reduction stays locked in."
            )
    for path in sorted(listed_map):
        if path not in actual_map:
            errors.append(
                f"[{sec}] {path}: 0 occurrences remain (allowlist has {listed_map[path]}). "
                f"Delete its entry from {allowlist_label}."
            )
    return errors


def compare_live_report(
    report: ScanReport, allowlist: Allowlist, allowlist_label: str = ALLOWLIST_REL
) -> tuple[list[str], list[str]]:
    errors, warnings = _check_crate_loc_limits(
        report.crate_locs, allowlist.crate_loc, allowlist_label
    )
    for sec in COUPLING_SECTIONS:
        errors.extend(
            _check_coupling_section(
                sec, report.coupling_counts[sec], allowlist.coupling[sec], allowlist_label
            )
        )
    return errors, warnings


def _diff_crate_loc_shrinks(before_loc: dict[str, int], after_loc: dict[str, int]) -> list[str]:
    problems: list[str] = []
    for crate, cap in sorted(after_loc.items()):
        if crate not in before_loc:
            problems.append(
                f"[crate_loc] `{crate}` ({cap}): new crates may not be added to the legacy monolith baseline"
            )
        elif cap > before_loc[crate]:
            problems.append(
                f"[crate_loc] `{crate}`: ceiling raised from {before_loc[crate]} to {cap}"
            )
    before_sum = sum(before_loc.values())
    after_sum = sum(after_loc.values())
    if after_sum > before_sum:
        problems.append(f"[crate_loc]: total ceiling raised from {before_sum} to {after_sum}")
    return problems


def _diff_coupling_shrinks(
    sec: str, before_map: dict[str, int], after_map: dict[str, int]
) -> list[str]:
    problems: list[str] = []
    added: list[str] = []
    for path, count in sorted(after_map.items()):
        if path not in before_map:
            added.append(f"[{sec}] {path}: added to allowlist ({count})")
        elif count > before_map[path]:
            problems.append(f"[{sec}] {path}: count raised from {before_map[path]} to {count}")
    b_total = sum(before_map.values())
    a_total = sum(after_map.values())
    if a_total > b_total:
        problems.extend(added)
        problems.append(f"[{sec}]: section total raised from {b_total} to {a_total}")
    return problems


def compare_allowlist_shrinks(
    before: Allowlist, after: Allowlist, base_ref: str, allowlist_label: str = ALLOWLIST_REL
) -> list[str]:
    """Verify `after` never raises any ceiling, coupling count, or section total vs `before`."""
    problems = _diff_crate_loc_shrinks(before.crate_loc, after.crate_loc)
    for sec in COUPLING_SECTIONS:
        problems.extend(_diff_coupling_shrinks(sec, before.coupling[sec], after.coupling[sec]))
    if problems:
        return [
            f"{allowlist_label} only shrinks compared with {base_ref}:",
            *(f"  {p}" for p in problems),
        ]
    return []


def _git_env() -> dict[str, str]:
    return {k: v for k, v in os.environ.items() if k not in GIT_REPO_ENV_VARS}


def read_allowlist_at_base(
    repo_root: Path, base: str, allowlist_rel: str = ALLOWLIST_REL
) -> str | None:
    if not _HEX_SHA_RE.fullmatch(base):
        raise ValueError(f"--base must be a lowercase hexadecimal commit SHA (7-64 chars), not {base!r}")

    env = _git_env()
    exists = subprocess.run(
        ["git", "-C", str(repo_root), "cat-file", "-e", f"{base}^{{commit}}"],
        capture_output=True,
        text=True,
        check=False,
        env=env,
    )
    if exists.returncode != 0:
        raise ValueError(f"base commit {base} is not available: {exists.stderr.strip()}")

    listed = subprocess.run(
        ["git", "-C", str(repo_root), "ls-tree", "--name-only", base, "--", allowlist_rel],
        capture_output=True,
        text=True,
        check=False,
        env=env,
    )
    if listed.returncode != 0:
        raise ValueError(f"git ls-tree {base}: {listed.stderr.strip()}")
    if not listed.stdout.strip():
        return None

    shown = subprocess.run(
        ["git", "-C", str(repo_root), "show", f"{base}:./{allowlist_rel}"],
        capture_output=True,
        text=True,
        check=False,
        env=env,
    )
    if shown.returncode != 0:
        raise ValueError(f"git show {base}:./{allowlist_rel}: {shown.stderr.strip()}")
    return shown.stdout


def format_baseline(report: ScanReport, crate_caps: dict[str, int]) -> str:
    lines: list[str] = [
        "# Architecture Health & Coupling Shrink-Only Ratchet Baseline (TASK-234 / P0-INT-4).",
        "#",
        "# Checked by `.github/scripts/check_architecture_health.py` on every CI run.",
        f"# Rules: `{RULES_DOC}`",
        "#",
        "# THIS FILE ONLY SHRINKS:",
        "# - `[crate_loc]` lists the legacy large crates with a strict shrink-only non-test LOC",
        "#   ceiling. No unlisted crate may exceed 30,000 non-test LOC (warns at >= 25,000),",
        "#   no listed crate may exceed its ceiling, and `--base <sha>` rejects adding any new",
        "#   crate or raising any ceiling. Lower a ceiling (or delete it once <= 25,000 LOC)",
        "#   as domains are extracted into cohesive crates.",
        "# - `[coupling.*]` lists non-adapter production Rust files that still reference concrete",
        "#   storage/transport/sandbox primitives or place `#[allow(dead_code)]` on a `trait`,",
        "#   each at its exact occurrence count. Lower counts (or delete at 0) in the same PR",
        "#   so wins stay permanently locked in. `--base <sha>` fails if any count or total rises.",
        "#",
        "# To print current measured entries in this format:",
        "#   python3 .github/scripts/check_architecture_health.py --print-baseline",
        "",
        f"[{SECTION_CRATE_LOC}]",
    ]
    for crate, cap in sorted(crate_caps.items()):
        lines.append(f"{crate} {cap}")
    for sec in COUPLING_SECTIONS:
        lines.append("")
        lines.append(f"[{sec}]")
        for path, count in sorted(report.coupling_counts[sec].items()):
            lines.append(f"{path} {count}")
    lines.append("")
    return "\n".join(lines)


# ---------------------------------------------------------------------------
# Top-level check runner & CLI
# ---------------------------------------------------------------------------


def run_health_check(
    repo_root: Path,
    allowlist_path: Path | None = None,
    base_commit: str | None = None,
) -> HealthResult:
    resolved_allowlist = allowlist_path or (repo_root / ALLOWLIST_REL)
    try:
        allowlist_label = resolved_allowlist.resolve().relative_to(repo_root.resolve()).as_posix()
    except ValueError:
        allowlist_label = resolved_allowlist.as_posix()

    members, workspace_dep_renames = _load_workspace(repo_root)
    report = scan_workspace(repo_root, members)
    dag_errors, layer0_checked = check_cargo_dag_and_layer0(
        members, workspace_dep_renames=workspace_dep_renames
    )
    hygiene_errors = check_workspace_hygiene(
        repo_root,
        members,
        workspace_dep_renames,
        iter_rs=_iter_crate_rs_files,
        is_test_path=is_test_source_path,
        mask_fn=mask_comments_and_strings,
        cfg_test_fn=find_cfg_test_ranges,
        prev_attr_fn=_prev_outer_attr_span,
        skip_balanced_fn=_skip_balanced,
    )

    if not resolved_allowlist.is_file():
        raise ValueError(f"missing allowlist file: {resolved_allowlist}")
    allowlist = parse_allowlist(
        resolved_allowlist.read_text(encoding="utf-8"), source_name=allowlist_label
    )

    live_errors, warnings = compare_live_report(report, allowlist, allowlist_label=allowlist_label)
    errors = [*dag_errors, *hygiene_errors, *live_errors]

    if base_commit is not None:
        base_text = read_allowlist_at_base(repo_root, base_commit, allowlist_rel=allowlist_label)
        if base_text is not None:
            before = parse_allowlist(base_text, source_name=f"{base_commit}:{allowlist_label}")
            errors.extend(
                compare_allowlist_shrinks(
                    before, allowlist, base_ref=base_commit, allowlist_label=allowlist_label
                )
            )

    return HealthResult(
        errors=errors,
        warnings=warnings,
        report=report,
        workspace_crates=[name for name, _, _ in members],
        layer0_checked=layer0_checked,
    )


def _build_arg_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Automated CI Architecture Health & Shrink-Only Coupling Guardrails."
    )
    parser.add_argument(
        "--repo-root",
        type=Path,
        default=Path(__file__).resolve().parents[2],
        help="Path to repository root (defaults to the repo containing this script).",
    )
    parser.add_argument(
        "--allowlist",
        type=Path,
        default=None,
        help=f"Path to allowlist file (defaults to <repo-root>/{ALLOWLIST_REL}).",
    )
    parser.add_argument(
        "--base",
        type=str,
        default=None,
        help="Lowercase hex commit SHA to enforce shrink-only allowlist diff against.",
    )
    parser.add_argument(
        "--print-baseline",
        action="store_true",
        help="Print current measured crate LOCs and coupling entries in allowlist format and exit 0.",
    )
    return parser


def _print_baseline_report(repo_root: Path, allowlist: Path | None) -> int:
    members = discover_workspace_members(repo_root)
    report = scan_workspace(repo_root, members)
    allowlist_file = allowlist or (repo_root / ALLOWLIST_REL)
    if allowlist_file.is_file() and allowlist_file.stat().st_size > 0:
        existing = parse_allowlist(allowlist_file.read_text(encoding="utf-8"))
        crate_caps = existing.crate_loc
    else:
        crate_caps = {
            crate: ((loc + 2_000 + 250) // 500) * 500
            for crate, loc in report.crate_locs.items()
            if loc >= CRATE_WARN_LOC
        }
    sys.stdout.write(format_baseline(report, crate_caps))
    return 0


def _emit_health_result(result: HealthResult, elapsed: float, base: str | None) -> int:
    for warning in result.warnings:
        print(warning, file=sys.stderr)
    if result.errors:
        print(
            f"Architecture health check FAILED ({len(result.errors)} error(s) in {elapsed:.2f}s):",
            file=sys.stderr,
        )
        for err in result.errors:
            print(f"  - {err}", file=sys.stderr)
        return 1
    coupling_summary = ", ".join(
        f"{sec.split('.', 1)[1]}={sum(result.report.coupling_counts[sec].values())}"
        for sec in COUPLING_SECTIONS
    )
    base_note = f"; shrink-only verified against {base[:12]}" if base else ""
    print(
        f"Architecture health OK: {len(result.workspace_crates)} crates, "
        f"{result.report.files_scanned} production .rs files, "
        f"0 DAG cycles, {len(result.layer0_checked)} Layer-0 crates clean, "
        f"0 unused deps, 0 orphan .rs files, "
        f"coupling({coupling_summary}){base_note} [{elapsed:.2f}s]."
    )
    return 0


def main(argv: list[str] | None = None) -> int:
    args = _build_arg_parser().parse_args(argv)
    t0 = time.perf_counter()
    try:
        if args.print_baseline:
            return _print_baseline_report(args.repo_root, args.allowlist)
        result = run_health_check(
            repo_root=args.repo_root,
            allowlist_path=args.allowlist,
            base_commit=args.base,
        )
    except ValueError as exc:
        print(f"ERROR: {exc}", file=sys.stderr)
        return 1
    return _emit_health_result(result, time.perf_counter() - t0, args.base)


if __name__ == "__main__":
    sys.exit(main())
