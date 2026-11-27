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
     a) `coupling.rusqlite`: `rusqlite` outside `crates/org-office/src/store/sqlite/`
        and `crates/org-host/src/hostdb/sqlite/`.
     b) `coupling.unix_socket`: `UnixStream` / `UnixListener` / `UnixDatagram` /
        `std::os::unix::net` / `tokio::net::Unix*` outside `crates/org-platform/`,
        `crates/org-platform-host/`, `crates/org-socket/`, and `crates/org-client/`.
     c) `coupling.sandbox`: `bwrap` / `sandbox-exec` process invocation string literals
        or Seatbelt/sandbox FFI/type references (`sandbox_init`,
        `sandbox_init_with_parameters`, `sandbox_check`, `sandbox_exec`, `Seatbelt`)
        outside `crates/org-platform/` and `crates/org-platform-host/`.
     d) `coupling.dead_code_trait`: `#[allow(...dead_code...)]` (including via
        `#[cfg_attr(..., allow(...dead_code...))]`) directly preceding any `trait`
        definition across all crates.
   - Enforces exact shrink-only agreement with `.github/architecture-boundaries.allow`:
     unlisted hits fail, raised counts fail, and reduced counts must be lowered (or
     deleted at 0) in the same change so wins stay permanently locked in.

3. Workspace Cargo DAG & Layer-0 Foundation Stability Guard:
   - Parses root `Cargo.toml` workspace members and each member's normal `[dependencies]`
     and `[target.*.dependencies]` (cleanly excluding `[dev-dependencies]`).
   - Verifies 0 cyclic dependencies across all workspace crates (reporting the exact
     cycle path on failure).
   - Verifies Layer-0 foundation crates (`org-proto`, `org-platform`, `org-pki`,
     `org-store`, `org-agent-runtime`, `org-command`, plus zero-dep leaf `org-docs`
     used by `org-command` for bounded YAML parsing) have zero normal `[dependencies]`
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

try:
    import tomllib  # Python 3.11+
except ModuleNotFoundError:  # pragma: no cover - fallback for Python < 3.11
    tomllib = None  # type: ignore[assignment]


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
    "crates/org-office/src/store/sqlite/",
    "crates/org-host/src/hostdb/sqlite/",
)
UNIX_SOCKET_ADAPTER_PREFIXES = (
    "crates/org-platform/",
    "crates/org-platform-host/",
    "crates/org-socket/",
    "crates/org-client/",
)
SANDBOX_ADAPTER_PREFIXES = (
    "crates/org-platform/",
    "crates/org-platform-host/",
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
        "org-store",
        "org-agent-runtime",
        "org-command",
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
    "sandbox",
    "Seatbelt",
    "dead_code",
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
    r"|\b(?:let|if|else|where|fn|mod|impl|struct|enum|trait|type|use|const|static|extern|macro_rules)\b"
)
_IDENT_AT_START_RE = re.compile(r"[A-Za-z_][A-Za-z0-9_]*")

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
    r"\b(?:sandbox_init|sandbox_init_with_parameters|sandbox_check|sandbox_exec|Seatbelt)\b"
)
# Matches unquoted string literals invoking bwrap or sandbox-exec as a binary/command,
# without false-positiving on English prose or diagnostic reason strings.
_SANDBOX_STR_VALUE_RE = re.compile(
    r"(?:^(?:[^\s\"']*/)?(?:bwrap|sandbox-exec)(?:$|\s))"
    r"|(?:(?<=[\s\"'])/(?:[^\s\"']*/)?(?:bwrap|sandbox-exec)(?:$|[\s\"']))"
)
_TRAIT_DECL_RE = re.compile(
    r"\b(?:pub(?:\s*\([^)]*\))?\s+)?(?:unsafe\s+)?(?:auto\s+)?trait\s+([A-Za-z_][A-Za-z0-9_]*)"
)
_ALLOW_CALL_RE = re.compile(r"\ballow\s*\(")
_DEAD_CODE_WORD_RE = re.compile(r"\bdead_code\b")
_RAW_STR_PREFIX_RE = re.compile(r"^(?:b|c)?r(#*)\"")
_PLAIN_STR_PREFIX_RE = re.compile(r"^(?:b|c)?\"")
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


# ---------------------------------------------------------------------------
# Minimal Cargo.toml loader (uses stdlib tomllib on 3.11+, pure-stdlib fallback)
# ---------------------------------------------------------------------------


def _parse_toml_fallback(text: str) -> dict[str, Any]:
    """Minimal pure-stdlib TOML parser covering Cargo.toml tables, arrays, and strings."""
    root: dict[str, Any] = {}
    current: dict[str, Any] = root
    lines = text.splitlines()
    i = 0

    def _strip_comment(line: str) -> str:
        in_str: str | None = None
        escaped = False
        for idx, ch in enumerate(line):
            if escaped:
                escaped = False
                continue
            if in_str == '"' and ch == "\\":
                escaped = True
                continue
            if ch in ('"', "'"):
                if in_str is None:
                    in_str = ch
                elif in_str == ch:
                    in_str = None
            elif ch == "#" and in_str is None:
                return line[:idx]
        return line

    def _split_dotted(header: str) -> list[str]:
        parts: list[str] = []
        buf: list[str] = []
        quote: str | None = None
        for ch in header:
            if quote:
                if ch == quote:
                    quote = None
                else:
                    buf.append(ch)
            elif ch in ('"', "'"):
                quote = ch
            elif ch == ".":
                parts.append("".join(buf).strip())
                buf = []
            else:
                buf.append(ch)
        if buf:
            parts.append("".join(buf).strip())
        return [p for p in parts if p]

    def _parse_value(raw: str) -> Any:
        raw = raw.strip()
        if not raw:
            return ""
        if raw.startswith('"') and raw.endswith('"'):
            return raw[1:-1]
        if raw.startswith("'") and raw.endswith("'"):
            return raw[1:-1]
        if raw == "true":
            return True
        if raw == "false":
            return False
        if raw.startswith("[") and raw.endswith("]"):
            inner = raw[1:-1]
            return [m.group(1) or m.group(2) for m in re.finditer(r'"([^"]*)"|\'([^\']*)\'', inner)]
        if raw.startswith("{") and raw.endswith("}"):
            tbl: dict[str, Any] = {}
            inner = raw[1:-1]
            for m in re.finditer(
                r'([A-Za-z0-9_-]+)\s*=\s*(?:"([^"]*)"|\'([^\']*)\'|(true|false))', inner
            ):
                k = m.group(1)
                if m.group(2) is not None:
                    tbl[k] = m.group(2)
                elif m.group(3) is not None:
                    tbl[k] = m.group(3)
                else:
                    tbl[k] = m.group(4) == "true"
            return tbl
        return raw

    while i < len(lines):
        line = _strip_comment(lines[i]).strip()
        i += 1
        if not line:
            continue
        if line.startswith("[") and not line.startswith("[[") and line.endswith("]"):
            path = _split_dotted(line[1:-1].strip())
            target = root
            for part in path:
                nxt = target.get(part)
                if not isinstance(nxt, dict):
                    nxt = {}
                    target[part] = nxt
                target = nxt
            current = target
            continue
        if "=" not in line:
            continue
        lhs, rhs = line.split("=", 1)
        rhs = rhs.strip()
        # Accumulate multi-line arrays / inline tables.
        while (rhs.startswith("[") and rhs.count("[") > rhs.count("]")) or (
            rhs.startswith("{") and rhs.count("{") > rhs.count("}")
        ):
            if i >= len(lines):
                break
            rhs += " " + _strip_comment(lines[i]).strip()
            i += 1
        keys = _split_dotted(lhs.strip())
        if not keys:
            continue
        target = current
        for part in keys[:-1]:
            nxt = target.get(part)
            if not isinstance(nxt, dict):
                nxt = {}
                target[part] = nxt
            target = nxt
        target[keys[-1]] = _parse_value(rhs)
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


def mask_comments_and_strings(src: str) -> tuple[str, list[tuple[int, int, str]]]:
    """Replace Rust comments, strings, and char literals with spaces preserving newlines.

    Returns `(code_only, string_spans)` where `string_spans` holds `(start, end, raw_literal)`
    for every non-comment string literal.
    """
    if "//" not in src and "/*" not in src and '"' not in src and "'" not in src:
        return src, []

    strings: list[tuple[int, int, str]] = []

    if "/*" not in src:
        # Fast C-loop path when no block comments exist.
        def _repl(m: re.Match[str]) -> str:
            if m.lastgroup in ("str", "raw_str"):
                strings.append((m.start(), m.end(), m.group(0)))
            return _blank_preserving_newlines(m.group(0))

        return _LEX_NO_BLOCK_RE.sub(_repl, src), strings

    n = len(src)
    out: list[str] = []
    pos = 0
    last = 0
    while pos < n:
        m = _LEX_RE.search(src, pos)
        if not m:
            break
        start = m.start()
        end = m.end()
        kind = m.lastgroup
        if kind == "block_comment":
            depth = 1
            cur = end
            while cur < n and depth > 0:
                nxt_open = src.find("/*", cur)
                nxt_close = src.find("*/", cur)
                if nxt_close == -1:
                    cur = n
                    break
                if nxt_open != -1 and nxt_open < nxt_close:
                    depth += 1
                    cur = nxt_open + 2
                else:
                    depth -= 1
                    cur = nxt_close + 2
            end = cur
        elif kind in ("str", "raw_str"):
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
            if depth > 0:
                depth -= 1
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
        item_start = m.start()
        # Locate end of `cfg(...)` and enclosing `#[...]`.
        paren_end = _skip_balanced(code, m.end(), "(", ")")
        pred = code[m.end() : paren_end - 1]
        bracket_pos = code.find("[", item_start)
        attr_end = _skip_balanced(code, bracket_pos + 1, "[", "]")
        # Ensure `cfg(...)` is the entire attribute (nothing extra besides whitespace before `]`).
        if code[paren_end : attr_end - 1].strip() != "" or not _is_test_only_cfg_predicate(pred):
            pos = attr_end
            continue

        # Skip any chained outer attributes `#[...]` and whitespace.
        p = attr_end
        while p < n:
            while p < n and code[p].isspace():
                p += 1
            if p < n and code[p] == "#":
                q = p + 1
                while q < n and code[q].isspace():
                    q += 1
                if q < n and code[q] == "[":
                    p = _skip_balanced(code, q + 1, "[", "]")
                    continue
            break

        # Determine attached item/statement/field kind and scan to its balanced end.
        seen_let = False
        seen_if = False
        in_where = False
        seen_item_kw = False
        paren = 0
        bracket = 0
        brace = 0
        angle = 0
        entered_brace = False

        wm = _IDENT_AT_START_RE.match(code, p)
        if wm:
            first_kw = wm.group(0)
            if first_kw == "let":
                seen_let = True
            elif first_kw == "if":
                seen_if = True
            elif first_kw in (
                "fn",
                "mod",
                "impl",
                "struct",
                "enum",
                "trait",
                "type",
                "use",
                "const",
                "static",
                "extern",
                "macro_rules",
            ):
                seen_item_kw = True

        cur = p
        while cur < n:
            tm = _STRUCT_TOK_RE.search(code, cur)
            if not tm:
                cur = n
                break
            tok = tm.group(0)
            if tok.startswith("#"):
                cur = _skip_balanced(code, tm.end(), "[", "]")
                continue
            if tok == "(":
                paren += 1
                cur = tm.end()
                continue
            if tok == ")":
                if paren > 0:
                    paren -= 1
                cur = tm.end()
                continue
            if tok == "[":
                bracket += 1
                cur = tm.end()
                continue
            if tok == "]":
                if bracket > 0:
                    bracket -= 1
                cur = tm.end()
                continue
            if tok == "<":
                if (
                    paren == 0
                    and bracket == 0
                    and brace == 0
                    and not seen_let
                    and not seen_if
                    and (tm.end() >= n or code[tm.end()] not in ("=", "<"))
                ):
                    angle += 1
                cur = tm.end()
                continue
            if tok == ">":
                if (
                    paren == 0
                    and bracket == 0
                    and brace == 0
                    and angle > 0
                    and (tm.start() == 0 or code[tm.start() - 1] not in ("-", "="))
                ):
                    angle -= 1
                cur = tm.end()
                continue
            if tok == "{":
                brace += 1
                entered_brace = True
                if paren == 0 and bracket == 0:
                    in_where = False
                    angle = 0
                cur = tm.end()
                continue
            if tok == "}":
                if brace == 0:
                    # Reached enclosing block's closing brace (e.g. last struct field without comma).
                    cur = tm.start()
                    break
                brace -= 1
                cur = tm.end()
                if brace == 0 and paren == 0 and bracket == 0 and entered_brace:
                    if seen_let:
                        continue
                    if seen_if:
                        q = cur
                        while q < n and code[q].isspace():
                            q += 1
                        if code[q : q + 4] == "else" and (
                            q + 4 == n or not (code[q + 4].isalnum() or code[q + 4] == "_")
                        ):
                            cur = q + 4
                            entered_brace = False
                            continue
                    break
                continue

            if paren == 0 and bracket == 0 and brace == 0:
                if tok == ";":
                    cur = tm.end()
                    break
                if tok == ",":
                    if (
                        angle == 0
                        and not seen_item_kw
                        and not in_where
                        and not seen_let
                        and not seen_if
                    ):
                        cur = tm.end()
                        break
                elif tok in (
                    "fn",
                    "mod",
                    "impl",
                    "struct",
                    "enum",
                    "trait",
                    "type",
                    "use",
                    "const",
                    "static",
                    "extern",
                    "macro_rules",
                ):
                    seen_item_kw = True
                elif tok == "where":
                    in_where = True
                elif tok == "let" and not seen_item_kw:
                    seen_let = True
            cur = tm.end()

        ranges.append((item_start, cur))
        pos = cur

    return ranges


def count_non_test_lines(src: str, ranges: list[tuple[int, int]]) -> int:
    """Count lines in `src` excluding lines wholly covered by `#[cfg(test)]` `ranges`."""
    if not src:
        return 0
    if not ranges:
        return src.count("\n") + (0 if src.endswith("\n") else 1)

    total = 0
    ri = 0
    nr = len(ranges)
    n = len(src)
    ls = 0
    while ls < n:
        nl = src.find("\n", ls)
        le = nl if nl != -1 else n
        while ri < nr and ranges[ri][1] <= ls:
            ri += 1
        if ri >= nr or ranges[ri][0] >= le:
            total += 1
        else:
            has_prod = False
            c = ls
            j = ri
            while j < nr and ranges[j][0] < le:
                rs, re_ = ranges[j]
                if c < rs and src[c : min(rs, le)].strip():
                    has_prod = True
                    break
                c = max(c, re_)
                j += 1
            if not has_prod and c < le and src[c:le].strip():
                has_prod = True
            if has_prod:
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


def _attr_contains_allow_dead_code(attr_text: str) -> bool:
    for m in _ALLOW_CALL_RE.finditer(attr_text):
        end = _skip_balanced(attr_text, m.end(), "(", ")")
        inner = attr_text[m.end() : end - 1]
        if _DEAD_CODE_WORD_RE.search(inner):
            return True
    return False


def find_dead_code_traits(prod_code: str) -> list[str]:
    """Return names of traits directly preceded by an outer `allow(...dead_code...)` attribute."""
    if "dead_code" not in prod_code or "trait" not in prod_code:
        return []
    hits: list[str] = []
    for tm in _TRAIT_DECL_RE.finditer(prod_code):
        idx = tm.start()
        attrs: list[str] = []
        while True:
            while idx > 0 and prod_code[idx - 1].isspace():
                idx -= 1
            if idx > 0 and prod_code[idx - 1] == "]":
                depth = 1
                j = idx - 2
                while j >= 0 and depth > 0:
                    ch = prod_code[j]
                    if ch == "]":
                        depth += 1
                    elif ch == "[":
                        depth -= 1
                    j -= 1
                if depth != 0:
                    break
                open_bracket = j + 1
                k = open_bracket - 1
                while k >= 0 and prod_code[k].isspace():
                    k -= 1
                # Outer attribute `#[...]` only (no `!` between `#` and `[`).
                if k >= 0 and prod_code[k] == "#" and prod_code[k + 1 : open_bracket].strip() == "":
                    attrs.append(prod_code[k:idx])
                    idx = k
                    continue
            break
        if any(_attr_contains_allow_dead_code(a) for a in attrs):
            hits.append(tm.group(1))
    return hits


def scan_production_file(
    rel_posix: str, text: str
) -> tuple[int, int, int, int, int]:
    """Return `(non_test_loc, rusqlite, unix_socket, sandbox, dead_code_trait)` for one file."""
    if not any(trigger in text for trigger in FAST_PATH_TRIGGERS):
        loc = text.count("\n") + (0 if not text or text.endswith("\n") else 1)
        return loc, 0, 0, 0, 0

    code, string_spans = mask_comments_and_strings(text)
    ranges = find_cfg_test_ranges(code)
    loc = count_non_test_lines(text, ranges)
    prod_code = blank_ranges(code, ranges)

    rusqlite_count = 0
    if not rel_posix.startswith(RUSQLITE_ADAPTER_PREFIXES) and "rusqlite" in prod_code:
        rusqlite_count = len(_RUSQLITE_RE.findall(prod_code))

    unix_count = 0
    if not rel_posix.startswith(UNIX_SOCKET_ADAPTER_PREFIXES) and (
        "Unix" in prod_code or "unix::net" in prod_code
    ):
        unix_count = len(_UNIX_SOCKET_RE.findall(prod_code))

    sandbox_count = 0
    if not rel_posix.startswith(SANDBOX_ADAPTER_PREFIXES):
        if "sandbox" in prod_code or "Seatbelt" in prod_code:
            sandbox_count += len(_SANDBOX_CODE_RE.findall(prod_code))
        if "bwrap" in text or "sandbox-exec" in text:
            for s_start, _s_end, raw_lit in string_spans:
                if ("bwrap" in raw_lit or "sandbox-exec" in raw_lit) and not _offset_in_ranges(
                    s_start, ranges
                ):
                    unquoted = _unquote_rust_string(raw_lit).strip()
                    if _SANDBOX_STR_VALUE_RE.search(unquoted):
                        sandbox_count += 1

    dead_code_trait_count = len(find_dead_code_traits(prod_code))
    return loc, rusqlite_count, unix_count, sandbox_count, dead_code_trait_count


# ---------------------------------------------------------------------------
# Workspace Cargo.toml discovery, DAG cycle check, and Layer-0 guard
# ---------------------------------------------------------------------------


def discover_workspace_members(repo_root: Path) -> list[tuple[str, Path, dict[str, Any]]]:
    """Return sorted `[(package_name, member_dir, manifest_dict), ...]` from root `Cargo.toml`."""
    root_manifest_path = repo_root / "Cargo.toml"
    if not root_manifest_path.is_file():
        raise ValueError(f"missing root Cargo.toml at {root_manifest_path}")
    root_manifest = load_toml(root_manifest_path)
    raw_members = (root_manifest.get("workspace") or {}).get("members") or []
    if not raw_members:
        raise ValueError("root Cargo.toml declares no [workspace].members")

    member_dirs: list[Path] = []
    seen_dirs: set[Path] = set()
    for pattern in raw_members:
        if any(ch in pattern for ch in "*?["):
            candidates = sorted(p for p in repo_root.glob(pattern) if (p / "Cargo.toml").is_file())
        else:
            candidates = [repo_root / pattern]
        for cand in candidates:
            resolved = cand.resolve()
            if resolved not in seen_dirs:
                seen_dirs.add(resolved)
                member_dirs.append(cand)

    members: list[tuple[str, Path, dict[str, Any]]] = []
    for member_dir in member_dirs:
        manifest_path = member_dir / "Cargo.toml"
        if not manifest_path.is_file():
            raise ValueError(f"workspace member missing Cargo.toml: {member_dir}")
        data = load_toml(manifest_path)
        pkg_name = (data.get("package") or {}).get("name")
        if not isinstance(pkg_name, str) or not pkg_name:
            raise ValueError(f"{manifest_path}: missing [package].name")
        members.append((pkg_name, member_dir, data))
    members.sort(key=lambda item: item[0])
    return members


def _extract_workspace_deps(dep_table: Any, workspace_pkgs: set[str]) -> set[str]:
    deps: set[str] = set()
    if not isinstance(dep_table, dict):
        return deps
    for key, val in dep_table.items():
        pkg_name = key
        if isinstance(val, dict) and isinstance(val.get("package"), str):
            pkg_name = val["package"]
        if pkg_name in workspace_pkgs:
            deps.add(pkg_name)
    return deps


def build_normal_dep_graph(
    members: list[tuple[str, Path, dict[str, Any]]]
) -> dict[str, list[str]]:
    """Build workspace normal dependency adjacency list (excluding dev-dependencies)."""
    workspace_pkgs = {name for name, _, _ in members}
    graph: dict[str, list[str]] = {}
    for name, _member_dir, data in members:
        deps = _extract_workspace_deps(data.get("dependencies"), workspace_pkgs)
        targets = data.get("target")
        if isinstance(targets, dict):
            for tdata in targets.values():
                if isinstance(tdata, dict):
                    deps |= _extract_workspace_deps(tdata.get("dependencies"), workspace_pkgs)
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
    members: list[tuple[str, Path, dict[str, Any]]]
) -> tuple[list[str], list[str]]:
    """Verify acyclic normal workspace DAG and Layer-0 foundation stability."""
    errors: list[str] = []
    graph = build_normal_dep_graph(members)

    for cycle in find_dependency_cycles(graph):
        errors.append(
            f"Cargo DAG cycle detected in normal [dependencies]: {' -> '.join(cycle)}"
        )

    workspace_pkgs = set(graph.keys())
    layer0_present = sorted(LAYER_0_ALLOWED_WORKSPACE_DEPS & workspace_pkgs)
    for crate in layer0_present:
        for dep in graph[crate]:
            if dep not in LAYER_0_ALLOWED_WORKSPACE_DEPS:
                errors.append(
                    f"Layer-0 stability violation: `{crate}` has a normal [dependencies] edge "
                    f"to non-Layer-0 crate `{dep}` (Layer-0 crates may only depend on std, "
                    f"external crates, or other Layer-0 crates)."
                )
    return errors, layer0_present


# ---------------------------------------------------------------------------
# Workspace source walk
# ---------------------------------------------------------------------------


def _iter_crate_rs_files(crate_dir: Path) -> list[Path]:
    found: list[Path] = []
    stack = [crate_dir]
    while stack:
        current = stack.pop()
        try:
            entries = list(os.scandir(current))
        except OSError as exc:
            raise ValueError(f"cannot read directory {current}: {exc}") from exc
        for entry in entries:
            name = entry.name
            if entry.is_dir(follow_symlinks=False):
                if (
                    name.startswith(".")
                    or name in SKIPPED_DIRS
                    or (Path(entry.path) / "CACHEDIR.TAG").exists()
                    or (Path(entry.path) / ".git").exists()
                ):
                    continue
                stack.append(Path(entry.path))
            elif name.endswith(".rs") and entry.is_file(follow_symlinks=True):
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
        if raw_line.startswith("[") and raw_line.endswith("]"):
            sec = raw_line[1:-1]
            if sec not in ALL_SECTIONS:
                raise ValueError(f"{source_name}:{lineno}: unknown section [{sec}]")
            if sec in seen_sections:
                raise ValueError(f"{source_name}:{lineno}: duplicate section [{sec}]")
            seen_sections.add(sec)
            current_section = sec
            prev_key = None
            continue
        if current_section is None:
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
        count = int(count_str)
        if prev_key is not None and prev_key >= key:
            raise ValueError(
                f"{source_name}:{lineno}: entries in [{current_section}] must be strictly sorted and unique"
            )
        prev_key = key

        if current_section == SECTION_CRATE_LOC:
            if not re.fullmatch(r"[A-Za-z0-9_-]+", key):
                raise ValueError(f"{source_name}:{lineno}: invalid crate name {key!r}")
            parsed.crate_loc[key] = count
        else:
            plain_path = (
                key.endswith(".rs")
                and "\\" not in key
                and not any(ch.isspace() for ch in key)
                and all(p not in ("", ".", "..") for p in key.split("/"))
            )
            if not plain_path:
                raise ValueError(
                    f"{source_name}:{lineno}: expected plain repo-relative `.rs` path, got {key!r}"
                )
            parsed.coupling[current_section][key] = count

    missing = [sec for sec in ALL_SECTIONS if sec not in seen_sections]
    if missing:
        raise ValueError(f"{source_name}: missing required section(s): {', '.join(missing)}")
    return parsed


def compare_live_report(
    report: ScanReport, allowlist: Allowlist, allowlist_label: str = ALLOWLIST_REL
) -> tuple[list[str], list[str]]:
    errors: list[str] = []
    warnings: list[str] = []

    # 1. Crate size ratchet
    for crate, loc in sorted(report.crate_locs.items()):
        if crate in allowlist.crate_loc:
            ceiling = allowlist.crate_loc[crate]
            if loc > ceiling:
                errors.append(
                    f"[crate_loc] `{crate}` has {loc:,} non-test LOC, exceeding its shrink-only "
                    f"baseline ceiling of {ceiling:,} in {allowlist_label}. Extract new domains "
                    f"into a cohesive crate ({RULES_DOC})."
                )
        else:
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

    for crate in sorted(allowlist.crate_loc):
        if crate not in report.crate_locs:
            errors.append(
                f"[crate_loc] `{crate}` in {allowlist_label} is no longer a workspace crate; "
                f"delete its entry."
            )

    # 2. Exact shrink-only coupling ratchets
    for sec in COUPLING_SECTIONS:
        actual_map = report.coupling_counts[sec]
        listed_map = allowlist.coupling[sec]
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

    return errors, warnings


def compare_allowlist_shrinks(
    before: Allowlist, after: Allowlist, base_ref: str, allowlist_label: str = ALLOWLIST_REL
) -> list[str]:
    """Verify `after` never raises any ceiling, coupling count, or section total vs `before`."""
    problems: list[str] = []

    # [crate_loc]: strictly shrink-only — never add a crate, never raise any ceiling or total.
    for crate, cap in sorted(after.crate_loc.items()):
        if crate not in before.crate_loc:
            problems.append(
                f"[crate_loc] `{crate}` ({cap}): new crates may not be added to the legacy monolith baseline"
            )
        elif cap > before.crate_loc[crate]:
            problems.append(
                f"[crate_loc] `{crate}`: ceiling raised from {before.crate_loc[crate]} to {cap}"
            )
    before_loc_sum = sum(before.crate_loc.values())
    after_loc_sum = sum(after.crate_loc.values())
    if after_loc_sum > before_loc_sum:
        problems.append(
            f"[crate_loc]: total ceiling raised from {before_loc_sum} to {after_loc_sum}"
        )

    # [coupling.*]: no listed path may rise, and no section total may rise (a new path is
    # accepted only when the section total does not rise, i.e. rename/split without adding).
    for sec in COUPLING_SECTIONS:
        b_map = before.coupling[sec]
        a_map = after.coupling[sec]
        added: list[str] = []
        for path, count in sorted(a_map.items()):
            if path not in b_map:
                added.append(f"[{sec}] {path}: added to allowlist ({count})")
            elif count > b_map[path]:
                problems.append(
                    f"[{sec}] {path}: count raised from {b_map[path]} to {count}"
                )
        b_total = sum(b_map.values())
        a_total = sum(a_map.values())
        if a_total > b_total:
            problems.extend(added)
            problems.append(f"[{sec}]: section total raised from {b_total} to {a_total}")

    if problems:
        return [f"{allowlist_label} only shrinks compared with {base_ref}:", *(f"  {p}" for p in problems)]
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

    members = discover_workspace_members(repo_root)
    report = scan_workspace(repo_root, members)
    dag_errors, layer0_checked = check_cargo_dag_and_layer0(members)

    if not resolved_allowlist.is_file():
        raise ValueError(f"missing allowlist file: {resolved_allowlist}")
    allowlist = parse_allowlist(
        resolved_allowlist.read_text(encoding="utf-8"), source_name=allowlist_label
    )

    live_errors, warnings = compare_live_report(report, allowlist, allowlist_label=allowlist_label)
    errors = [*dag_errors, *live_errors]

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


def main(argv: list[str] | None = None) -> int:
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
    args = parser.parse_args(argv)
    t0 = time.perf_counter()

    try:
        if args.print_baseline:
            members = discover_workspace_members(args.repo_root)
            report = scan_workspace(args.repo_root, members)
            allowlist_file = args.allowlist or (args.repo_root / ALLOWLIST_REL)
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

        result = run_health_check(
            repo_root=args.repo_root,
            allowlist_path=args.allowlist,
            base_commit=args.base,
        )
    except ValueError as exc:
        print(f"ERROR: {exc}", file=sys.stderr)
        return 1

    elapsed = time.perf_counter() - t0
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
    base_note = f"; shrink-only verified against {args.base[:12]}" if args.base else ""
    print(
        f"Architecture health OK: {len(result.workspace_crates)} crates, "
        f"{result.report.files_scanned} production .rs files, "
        f"0 DAG cycles, {len(result.layer0_checked)} Layer-0 crates clean, "
        f"coupling({coupling_summary}){base_note} [{elapsed:.2f}s]."
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
