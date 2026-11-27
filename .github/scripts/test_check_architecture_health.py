#!/usr/bin/env python3
"""Unit tests for .github/scripts/check_architecture_health.py (TASK-234 / P0-INT-4)."""

from __future__ import annotations

import importlib
import io
import os
import subprocess
import sys
import tempfile
import unittest
from contextlib import redirect_stderr, redirect_stdout
from pathlib import Path

SCRIPTS_DIR = Path(__file__).resolve().parent
if str(SCRIPTS_DIR) not in sys.path:
    sys.path.insert(0, str(SCRIPTS_DIR))

health = importlib.import_module("check_architecture_health")


ALLOWLIST_HEADER = "# Synthetic architecture boundaries (only shrinks).\n"


def _write(root: Path, rel: str, content: str) -> Path:
    path = root / rel
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content, encoding="utf-8")
    return path


def _make_allowlist(
    crate_loc: dict[str, int] | None = None,
    rusqlite: dict[str, int] | None = None,
    unix_socket: dict[str, int] | None = None,
    sandbox: dict[str, int] | None = None,
    dead_code_trait: dict[str, int] | None = None,
) -> str:
    sections = [
        ("crate_loc", crate_loc or {}),
        ("coupling.rusqlite", rusqlite or {}),
        ("coupling.unix_socket", unix_socket or {}),
        ("coupling.sandbox", sandbox or {}),
        ("coupling.dead_code_trait", dead_code_trait or {}),
    ]
    lines = [ALLOWLIST_HEADER]
    for sec, entries in sections:
        lines.append(f"[{sec}]")
        for key, val in sorted(entries.items()):
            lines.append(f"{key} {val}")
        lines.append("")
    return "\n".join(lines)


def _create_workspace(
    root: Path,
    members: list[str],
    crates: dict[str, tuple[str, dict[str, str]]],
    allowlist_text: str | None = None,
    root_extra: str = "",
) -> None:
    """Create a synthetic Cargo workspace under `root`.

    `crates` maps member path (e.g. `"crates/org-proto"`) to
    `(cargo_toml_content, {relative_file_path: content})`.
    """
    member_list = ",\n".join(f'  "{m}"' for m in members)
    root_toml = f'[workspace]\nresolver = "2"\nmembers = [\n{member_list}\n]\n'
    if root_extra:
        root_toml += "\n" + root_extra
    _write(root, "Cargo.toml", root_toml)
    for member, (manifest, files) in crates.items():
        _write(root, f"{member}/Cargo.toml", manifest)
        for rel_file, content in files.items():
            _write(root, f"{member}/{rel_file}", content)
    _write(
        root,
        health.ALLOWLIST_REL,
        allowlist_text if allowlist_text is not None else _make_allowlist(),
    )


def _git(root: Path, *args: str) -> str:
    env = {
        k: v
        for k, v in os.environ.items()
        if k not in health.GIT_REPO_ENV_VARS
    }
    env["GIT_AUTHOR_NAME"] = "Test"
    env["GIT_AUTHOR_EMAIL"] = "test@example.com"
    env["GIT_COMMITTER_NAME"] = "Test"
    env["GIT_COMMITTER_EMAIL"] = "test@example.com"
    proc = subprocess.run(
        ["git", "-C", str(root), *args],
        capture_output=True,
        text=True,
        check=True,
        env=env,
    )
    return proc.stdout.strip()


class TestCrateSizeRatchet(unittest.TestCase):
    def test_unlisted_crate_warns_at_25k_and_passes(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            # 25,000 non-test lines + 6,000 test lines (excluded) => 25,000 non-test LOC -> warn only.
            prod = "pub fn a() {}\n" * 25_000
            inline_tests = "#[cfg(test)]\nmod tests {\n" + ("    fn t() {}\n" * 6_000) + "}\n"
            _create_workspace(
                root,
                ["crates/org-alpha"],
                {
                    "crates/org-alpha": (
                        '[package]\nname = "org-alpha"\nversion = "0.0.0"\n',
                        {
                            "src/lib.rs": prod + inline_tests,
                            "tests/integration.rs": "fn it() {}\n" * 10_000,
                            "src/foo_tests.rs": "fn ft() {}\n" * 10_000,
                            "src/tests.rs": "fn ts() {}\n" * 10_000,
                            "src/testkit.rs": "fn tk() {}\n" * 10_000,
                            "src/testkit_helpers/mod.rs": "fn tkh() {}\n" * 10_000,
                        },
                    )
                },
            )
            res = health.run_health_check(root)
            self.assertEqual(res.report.crate_locs["org-alpha"], 25_000)
            self.assertEqual(res.errors, [])
            self.assertEqual(len(res.warnings), 1)
            self.assertIn("org-alpha", res.warnings[0])
            self.assertIn("25,000", res.warnings[0])

    def test_unlisted_crate_fails_above_30k(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            _create_workspace(
                root,
                ["crates/org-monolith"],
                {
                    "crates/org-monolith": (
                        '[package]\nname = "org-monolith"\nversion = "0.0.0"\n',
                        {"src/lib.rs": "pub fn f() {}\n" * 30_001},
                    )
                },
            )
            res = health.run_health_check(root)
            self.assertEqual(res.report.crate_locs["org-monolith"], 30_001)
            self.assertTrue(any("org-monolith" in e and "30,000" in e for e in res.errors))

    def test_listed_crate_respects_baseline_ceiling_and_fails_above_it(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            _create_workspace(
                root,
                ["crates/org-office"],
                {
                    "crates/org-office": (
                        '[package]\nname = "org-office"\nversion = "0.0.0"\n',
                        {"src/lib.rs": "pub fn f() {}\n" * 35_000},
                    )
                },
                allowlist_text=_make_allowlist(crate_loc={"org-office": 35_000}),
            )
            ok = health.run_health_check(root)
            self.assertEqual(ok.errors, [])
            self.assertEqual(ok.warnings, [])

            # Exceed baseline ceiling by 1 LOC -> hard failure.
            _write(root, "crates/org-office/src/lib.rs", "pub fn f() {}\n" * 35_001)
            bad = health.run_health_check(root)
            self.assertTrue(any("org-office" in e and "35,000" in e for e in bad.errors))

    def test_cfg_test_stripping_handles_complex_rust_syntax(self) -> None:
        src = """\
// Normal production line with /* comment marker and "raw"
pub struct Holder {
    pub live: usize,
    #[cfg(test)]
    pub map: std::collections::HashMap<String, UnixStream>,
    #[cfg(all(test, target_os = "linux"))]
    pub hook: Option<fn()>,
}

pub fn prod() -> char {
    let q = '\\'';
    let raw = r#"#[cfg(test)] mod not_really_test { }"#;
    /* nested /* block comment #[cfg(test)] */ still comment */
    let _ = raw;
    #[cfg(test)]
    if true {
        use rusqlite::Connection;
    } else {
        let _ = "bwrap";
    }
    #[cfg(test)]
    let _x = if true { 1 } else { 2 };
    q
}

#[cfg(test)]
#[allow(dead_code)]
mod tests {
    use rusqlite::Connection;
    use std::os::unix::net::UnixStream;

    #[allow(dead_code)]
    trait TestOnlyPort {}
}
"""
        loc, rusq, unix, sbox, dct = health.scan_production_file("crates/org-demo/src/lib.rs", src)
        self.assertEqual(rusq, 0)
        self.assertEqual(unix, 0)
        self.assertEqual(sbox, 0)
        self.assertEqual(dct, 0)
        # Lines outside #[cfg(test)] blocks:
        # 1..3 (3 lines), 8 (}), 9 (blank), 10..14 (5 lines), 23 (q), 24 (}), 25 (blank) = 13 lines.
        self.assertEqual(loc, 13)

    def test_cfg_test_if_let_and_while_let_do_not_swallow_following_prod_code(self) -> None:
        src = """\
fn run() {
    #[cfg(test)]
    if let Some(x) = opt() {
        let _ = x;
    }
    use rusqlite::Connection;
    #[cfg(test)]
    while let Some(y) = iter.next() {
        let _ = y;
    }
    let _ = UnixStream::connect("/tmp/sock");
    #[cfg(test)]
    if let Some(z) = opt2() {
        let _ = z;
    } else {
        let _ = 0;
    }
    let _ = rusqlite::Connection::open_in_memory();
}
"""
        loc, rusq, unix, sbox, dct = health.scan_production_file("crates/org-demo/src/lib.rs", src)
        self.assertEqual(loc, 5)
        self.assertEqual(rusq, 2)
        self.assertEqual(unix, 1)
        self.assertEqual(sbox, 0)
        self.assertEqual(dct, 0)

    def test_cfg_test_preceding_outer_attributes_stripped_without_leaking(self) -> None:
        src = (
            "#[allow(dead_code)]\n"
            "#[cfg(test)]\n"
            "fn test_helper() {}\n"
            "\n"
            "pub trait RealLivePort {}\n"
        )
        loc, rusq, unix, sbox, dct = health.scan_production_file("crates/org-demo/src/lib.rs", src)
        self.assertEqual(dct, 0)
        self.assertEqual(loc, 2)
        self.assertEqual((rusq, unix, sbox), (0, 0, 0))

        # Inner `#![...]` attribute stays intact while outer attrs + intervening comments
        # and preceding non-test `#[cfg(...)]` on the same test item are stripped cleanly.
        src_with_inner = (
            "#![allow(dead_code)]\n"
            "#[allow(dead_code)]\n"
            "// note between outer attributes\n"
            "#[cfg(unix)]\n"
            "#[cfg(test)]\n"
            "fn unix_test_helper() {}\n"
            "\n"
            "pub trait RealLivePort {}\n"
        )
        loc2, _r, _u, _s, dct2 = health.scan_production_file(
            "crates/org-demo/src/lib.rs", src_with_inner
        )
        self.assertEqual(dct2, 0)
        self.assertEqual(loc2, 3)

    def test_cfg_test_use_group_and_brace_bearing_const_static_stop_at_semicolon(self) -> None:
        src = """\
#[cfg(test)]
use rusqlite::{Connection, OpenFlags};
#[cfg(test)]
pub use std::os::unix::net::{UnixListener, UnixStream};
#[cfg(test)]
const X: usize = { 1 } + { 2 };
#[cfg(test)]
static S: Foo = Foo { a: 1 };
#[cfg(test)]
const fn test_const_fn() -> usize {
    1
}
#[cfg(test)]
const F: fn() = || {
    let _ = 1;
};
use rusqlite::Connection;
pub fn live() {
    let _ = UnixStream::connect("/tmp/a");
}
"""
        code, _ = health.mask_comments_and_strings(src)
        ranges = health.find_cfg_test_ranges(code)
        prod_code = health.blank_ranges(code, ranges)
        # No stray trailing `;` or `+ { 2 };` leaked before production `use rusqlite::Connection;`.
        self.assertTrue(prod_code.lstrip().startswith("use rusqlite::Connection;"))

        loc, rusq, unix, sbox, dct = health.scan_production_file("crates/org-demo/src/lib.rs", src)
        self.assertEqual(loc, 4)
        self.assertEqual(rusq, 1)
        self.assertEqual(unix, 1)
        self.assertEqual(sbox, 0)
        self.assertEqual(dct, 0)


class TestShrinkOnlyBaseEnforcement(unittest.TestCase):
    def _init_repo(self, root: Path, base_allow: str) -> str:
        _create_workspace(
            root,
            ["crates/org-host", "crates/org-proto"],
            {
                "crates/org-host": (
                    '[package]\nname = "org-host"\nversion = "0.0.0"\n',
                    {
                        "src/a.rs": "use std::os::unix::net::UnixStream;\nfn f(_: UnixStream) {}\n",
                        "src/b.rs": "use std::os::unix::net::UnixStream;\nfn g() {}\n",
                    },
                ),
                "crates/org-proto": (
                    '[package]\nname = "org-proto"\nversion = "0.0.0"\n',
                    {"src/lib.rs": "pub struct Id;\n"},
                ),
            },
            allowlist_text=base_allow,
        )
        _git(root, "init", "-q")
        _git(root, "add", ".")
        _git(root, "commit", "-q", "-m", "base")
        return _git(root, "rev-parse", "HEAD")

    def test_base_rejects_raising_crate_ceiling_or_adding_new_crate(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            base_allow = _make_allowlist(
                crate_loc={"org-host": 45_000},
                unix_socket={"crates/org-host/src/a.rs": 2, "crates/org-host/src/b.rs": 1},
            )
            base_sha = self._init_repo(root, base_allow)

            # Raising org-host ceiling -> rejected.
            _write(
                root,
                health.ALLOWLIST_REL,
                _make_allowlist(
                    crate_loc={"org-host": 45_500},
                    unix_socket={"crates/org-host/src/a.rs": 2, "crates/org-host/src/b.rs": 1},
                ),
            )
            res = health.run_health_check(root, base_commit=base_sha)
            self.assertTrue(any("ceiling raised" in e for e in res.errors))

            # Adding a new crate even while shrinking org-host -> rejected.
            _write(
                root,
                health.ALLOWLIST_REL,
                _make_allowlist(
                    crate_loc={"org-host": 40_000, "org-proto": 2_000},
                    unix_socket={"crates/org-host/src/a.rs": 2, "crates/org-host/src/b.rs": 1},
                ),
            )
            res2 = health.run_health_check(root, base_commit=base_sha)
            self.assertTrue(any("new crates may not be added" in e for e in res2.errors))

    def test_base_rejects_raising_coupling_or_adding_entries_increasing_total(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            base_allow = _make_allowlist(
                crate_loc={"org-host": 45_000},
                unix_socket={"crates/org-host/src/a.rs": 2, "crates/org-host/src/b.rs": 1},
            )
            base_sha = self._init_repo(root, base_allow)

            # Raise a.rs from 2 to 3 while shrinking b.rs from 1 to 0 (total stays 3):
            # still rejected because an existing path's count rose!
            _write(
                root,
                "crates/org-host/src/a.rs",
                "use std::os::unix::net::UnixStream;\nfn f(_: UnixStream, _: UnixStream) {}\n",
            )
            _write(root, "crates/org-host/src/b.rs", "fn g() {}\n")
            _write(
                root,
                health.ALLOWLIST_REL,
                _make_allowlist(
                    crate_loc={"org-host": 45_000},
                    unix_socket={"crates/org-host/src/a.rs": 3},
                ),
            )
            res = health.run_health_check(root, base_commit=base_sha)
            self.assertTrue(any("count raised from 2 to 3" in e for e in res.errors))

            # Add brand new coupling file raising section total from 3 to 4 -> rejected.
            _write(
                root,
                "crates/org-host/src/a.rs",
                "use std::os::unix::net::UnixStream;\nfn f(_: UnixStream) {}\n",
            )
            _write(
                root,
                "crates/org-host/src/b.rs",
                "use std::os::unix::net::UnixStream;\nfn g() {}\n",
            )
            _write(
                root,
                "crates/org-host/src/c.rs",
                "use std::os::unix::net::UnixListener;\n",
            )
            _write(
                root,
                health.ALLOWLIST_REL,
                _make_allowlist(
                    crate_loc={"org-host": 45_000},
                    unix_socket={
                        "crates/org-host/src/a.rs": 2,
                        "crates/org-host/src/b.rs": 1,
                        "crates/org-host/src/c.rs": 1,
                    },
                ),
            )
            res2 = health.run_health_check(root, base_commit=base_sha)
            self.assertTrue(any("section total raised from 3 to 4" in e for e in res2.errors))

    def test_base_allows_shrinking_and_moving_without_increasing_totals(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            base_allow = _make_allowlist(
                crate_loc={"org-host": 45_000},
                unix_socket={"crates/org-host/src/a.rs": 2, "crates/org-host/src/b.rs": 1},
            )
            base_sha = self._init_repo(root, base_allow)

            # Shrink org-host ceiling (45000 -> 44000) and rename/split a.rs (2 -> 1) into a_part.rs (1)
            # while deleting b.rs (1 -> 0): total drops 3 -> 2, no existing path rises -> PASS!
            _write(root, "crates/org-host/src/a.rs", "use std::os::unix::net::UnixStream;\n")
            _write(root, "crates/org-host/src/a_part.rs", "use std::os::unix::net::UnixStream;\n")
            _write(root, "crates/org-host/src/b.rs", "pub fn clean() {}\n")
            _write(
                root,
                health.ALLOWLIST_REL,
                _make_allowlist(
                    crate_loc={"org-host": 44_000},
                    unix_socket={
                        "crates/org-host/src/a.rs": 1,
                        "crates/org-host/src/a_part.rs": 1,
                    },
                ),
            )
            res = health.run_health_check(root, base_commit=base_sha)
            self.assertEqual(res.errors, [])

    def test_base_passes_when_allowlist_did_not_exist_at_base(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            _write(root, "Cargo.toml", '[workspace]\nmembers = ["crates/org-proto"]\n')
            _write(root, "crates/org-proto/Cargo.toml", '[package]\nname = "org-proto"\nversion = "0.0.0"\n')
            _write(root, "crates/org-proto/src/lib.rs", "pub struct X;\n")
            _git(root, "init", "-q")
            _git(root, "add", ".")
            _git(root, "commit", "-q", "-m", "before allowlist")
            base_sha = _git(root, "rev-parse", "HEAD")

            _write(root, health.ALLOWLIST_REL, _make_allowlist())
            res = health.run_health_check(root, base_commit=base_sha)
            self.assertEqual(res.errors, [])


class TestInfraCouplingDetection(unittest.TestCase):
    def test_rusqlite_unix_and_sandbox_detected_outside_adapters_and_allowed_inside(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            _create_workspace(
                root,
                [
                    "crates/org-office",
                    "crates/org-office-sqlite",
                    "crates/org-host-sqlite",
                    "crates/org-platform",
                    "crates/org-socket",
                    "crates/org-client",
                ],
                {
                    "crates/org-office-sqlite": (
                        '[package]\nname = "org-office-sqlite"\nversion = "0.0.0"\n',
                        {
                            # Allowed inside extracted SQLite adapter crate:
                            "src/conn.rs": "use rusqlite::Connection;\n",
                        },
                    ),
                    "crates/org-office": (
                        '[package]\nname = "org-office"\nversion = "0.0.0"\n',
                        {
                            # Violating outside adapter:
                            "src/domain.rs": (
                                "use rusqlite::Connection;\n"
                                "use tokio::net::UnixStream;\n"
                                'fn run() { let _ = std::process::Command::new("bwrap"); }\n'
                                'fn mac() { let _ = std::process::Command::new("/usr/bin/sandbox-exec"); }\n'
                            ),
                            # Prose reason string mentioning bwrap + comment + #[cfg(test)] must be ignored:
                            "src/clean.rs": (
                                '// rusqlite UnixStream bwrap sandbox-exec Seatbelt\n'
                                '#[allow(clippy::too_many_arguments, reason = "takes bwrap flag")]\n'
                                "pub fn ok() {}\n"
                                "#[cfg(test)]\nmod tests {\n"
                                "    use rusqlite::Connection;\n"
                                "    use std::os::unix::net::UnixListener;\n"
                                '    fn t() { let _ = "sandbox-exec"; }\n'
                                "}\n"
                            ),
                            # Dedicated test file must be ignored:
                            "tests/e2e.rs": (
                                "use rusqlite::Connection;\n"
                                "use std::os::unix::net::UnixStream;\n"
                                'fn t() { let _ = "bwrap"; }\n'
                            ),
                        },
                    ),
                    "crates/org-host-sqlite": (
                        '[package]\nname = "org-host-sqlite"\nversion = "0.0.0"\n',
                        {"src/lib.rs": "use rusqlite::Connection;\n"},
                    ),
                    "crates/org-platform": (
                        '[package]\nname = "org-platform"\nversion = "0.0.0"\n',
                        {
                            "src/confinement/macos.rs": (
                                "use std::os::unix::net::UnixStream;\n"
                                "extern \"C\" { fn sandbox_init_with_parameters(); }\n"
                                'const BIN: &str = "sandbox-exec";\n'
                            )
                        },
                    ),
                    "crates/org-socket": (
                        '[package]\nname = "org-socket"\nversion = "0.0.0"\n',
                        {"src/peer.rs": "use std::os::unix::net::UnixStream;\n"},
                    ),
                    "crates/org-client": (
                        '[package]\nname = "org-client"\nversion = "0.0.0"\n',
                        {"src/transport.rs": "use std::os::unix::net::UnixStream;\n"},
                    ),
                },
            )
            res = health.run_health_check(root)
            self.assertEqual(
                res.report.coupling_counts[health.SECTION_RUSQLITE],
                {"crates/org-office/src/domain.rs": 1},
            )
            self.assertEqual(
                res.report.coupling_counts[health.SECTION_UNIX_SOCKET],
                {"crates/org-office/src/domain.rs": 1},
            )
            self.assertEqual(
                res.report.coupling_counts[health.SECTION_SANDBOX],
                {"crates/org-office/src/domain.rs": 2},
            )
            self.assertEqual(len(res.errors), 3)

    def test_stale_coupling_allowlist_entry_fails_until_lowered(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            _create_workspace(
                root,
                ["crates/org-host"],
                {
                    "crates/org-host": (
                        '[package]\nname = "org-host"\nversion = "0.0.0"\n',
                        {"src/a.rs": "use std::os::unix::net::UnixStream;\n"},
                    )
                },
                allowlist_text=_make_allowlist(unix_socket={"crates/org-host/src/a.rs": 2}),
            )
            res = health.run_health_check(root)
            self.assertTrue(any("Lower its entry" in e for e in res.errors))

    def test_tightened_sandbox_detector_catches_bwrap_and_seatbelt_identifiers_outside_adapters(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            _create_workspace(
                root,
                [
                    "crates/org-office-sessions",
                    "crates/org-agent-runtime-claude",
                    "apps/agentic-org-web",
                ],
                {
                    "crates/org-office-sessions": (
                        '[package]\nname = "org-office-sessions"\nversion = "0.0.0"\n',
                        {
                            # Designated sandbox adapter: bwrap/Seatbelt identifiers allowed.
                            "src/manager/sandbox_runner/bwrap_isolation.rs": (
                                "pub fn ensure_bwrap_deny_dirs() {}\n"
                                "pub struct SeatbeltProfile;\n"
                            ),
                            # Non-adapter file: concrete bwrap/Seatbelt identifiers MUST be caught,
                            # including mixed-case identifiers that miss case-sensitive triggers.
                            "src/manager/leaked.rs": (
                                "pub fn bwrap_foo() {}\n"
                                "pub struct BwrapIsolation;\n"
                                "pub struct BWrapRunner;\n"
                                "pub fn my_bWrap_fn() {}\n"
                                "pub const BWRAP_BIN: &str = \"x\";\n"
                                "pub struct SeatbeltProfile;\n"
                                "pub struct SeatBeltProfile;\n"
                                "pub fn use_seatBelt() {}\n"
                                "pub fn seatbelt_rules() {}\n"
                            ),
                            # Comments and prose strings mentioning bwrap/seatbelt stay clean.
                            "src/manager/clean.rs": (
                                "// bwrap_foo SeatbeltProfile BWRAP_BIN\n"
                                'pub const DOC: &str = "mentions bwrap and seatbelt prose";\n'
                                "#[cfg(test)]\nmod tests {\n"
                                "    fn bwrap_test_only() {}\n"
                                "    struct SeatbeltMock;\n"
                                "}\n"
                            ),
                        },
                    ),
                    "crates/org-agent-runtime-claude": (
                        '[package]\nname = "org-agent-runtime-claude"\nversion = "0.0.0"\n',
                        {
                            "src/plan/sandbox.rs": "pub fn normalize_bwrap_pattern() {}\n",
                            "src/plan.rs": "pub fn format_bwrap_rule() {}\n",
                        },
                    ),
                    "apps/agentic-org-web": (
                        '[package]\nname = "agentic-org-web"\nversion = "0.0.0"\n',
                        {
                            # Designated web Unix socket adapter: exact file match allowed.
                            "src/transport/local_unix.rs": (
                                "use tokio::net::UnixStream;\n"
                                "pub fn dial(_: UnixStream) {}\n"
                            ),
                            # Prefix lookalike file MUST NOT be exempted by exact file entry.
                            "src/transport/local_unix.rs.extra.rs": (
                                "use tokio::net::UnixStream;\n"
                                "pub fn leak(_: UnixStream) {}\n"
                            ),
                        },
                    ),
                },
            )
            res = health.run_health_check(root)
            self.assertEqual(
                res.report.coupling_counts[health.SECTION_SANDBOX],
                {
                    "crates/org-office-sessions/src/manager/leaked.rs": 9,
                    "crates/org-agent-runtime-claude/src/plan.rs": 1,
                },
            )
            self.assertEqual(
                res.report.coupling_counts[health.SECTION_UNIX_SOCKET],
                {"apps/agentic-org-web/src/transport/local_unix.rs.extra.rs": 2},
            )
            self.assertTrue(
                any("coupling.sandbox" in e and "leaked.rs" in e for e in res.errors)
            )


class TestDeadCodeOnTraitGuard(unittest.TestCase):
    def test_dead_code_on_trait_fails_unlisted_passes_listed_and_ignores_non_traits(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            src = """\
#![allow(dead_code)]

#[allow(dead_code)]
fn helper_fn() {}

#[allow(dead_code)]
struct HelperStruct;

#[allow(dead_code)]
pub trait UnlistedPort {
    fn ping(&self);
}

#[cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "temporary wiring"
    )
)]
/// Doc comment between attribute and trait
pub(crate) trait DeferredStore {
    fn save(&self);
}

#[cfg(test)]
mod tests {
    #[allow(dead_code)]
    trait TestMockPort {}
}
"""
            _create_workspace(
                root,
                ["crates/org-host"],
                {
                    "crates/org-host": (
                        '[package]\nname = "org-host"\nversion = "0.0.0"\n',
                        {"src/hostdb.rs": src},
                    )
                },
            )
            # Unlisted -> fails with count 2 (UnlistedPort + DeferredStore).
            res = health.run_health_check(root)
            self.assertEqual(
                res.report.coupling_counts[health.SECTION_DEAD_CODE_TRAIT],
                {"crates/org-host/src/hostdb.rs": 2},
            )
            self.assertTrue(any("coupling.dead_code_trait" in e for e in res.errors))

            # Listed at exact count 2 -> passes.
            _write(
                root,
                health.ALLOWLIST_REL,
                _make_allowlist(dead_code_trait={"crates/org-host/src/hostdb.rs": 2}),
            )
            ok = health.run_health_check(root)
            self.assertEqual(ok.errors, [])

    def test_expect_dead_code_and_compact_pub_crate_trait_caught(self) -> None:
        src = """\
#[expect(dead_code, reason = "deferred wiring")]
pub trait ExpectedPort {
    fn ping(&self);
}

#[cfg_attr(not(test), expect(dead_code, reason = "pending adapter"))]
pub(crate)trait CompactVisPort {
    fn save(&self);
}

#[allow(dead_code)]
pub(super)trait CompactSuperPort {}

#[expect(clippy::too_many_arguments, reason = "mentions dead_code in prose")]
pub trait CleanPort {}
"""
        code, _ = health.mask_comments_and_strings(src)
        hits = health.find_dead_code_traits(code)
        self.assertEqual(hits, ["ExpectedPort", "CompactVisPort", "CompactSuperPort"])

        loc, rusq, unix, sbox, dct = health.scan_production_file("crates/org-host/src/ports.rs", src)
        self.assertEqual(dct, 3)
        self.assertEqual((rusq, unix, sbox), (0, 0, 0))
        self.assertGreater(loc, 0)


class TestCargoDagAndLayer0(unittest.TestCase):
    def test_detects_normal_dependency_cycle_and_ignores_dev_dependency_cycle(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            _create_workspace(
                root,
                ["crates/org-a", "crates/org-b"],
                {
                    "crates/org-a": (
                        '[package]\nname = "org-a"\nversion = "0.0.0"\n'
                        '[dependencies]\norg-b = { path = "../org-b" }\n',
                        {"src/lib.rs": ""},
                    ),
                    "crates/org-b": (
                        '[package]\nname = "org-b"\nversion = "0.0.0"\n'
                        '[dependencies]\norg-a = { path = "../org-a" }\n',
                        {"src/lib.rs": ""},
                    ),
                },
            )
            res = health.run_health_check(root)
            self.assertTrue(
                any("Cargo DAG cycle" in e and "org-a -> org-b -> org-a" in e for e in res.errors),
                f"unexpected errors: {res.errors}",
            )

            # Move org-b -> org-a into [dev-dependencies] (plus self-dev-dep) -> acyclic!
            _write(
                root,
                "crates/org-b/Cargo.toml",
                '[package]\nname = "org-b"\nversion = "0.0.0"\n'
                '[dev-dependencies]\norg-a = { path = "../org-a" }\n'
                'org-b = { path = ".", features = ["testkit"] }\n',
            )
            ok = health.run_health_check(root)
            self.assertEqual(ok.errors, [])

    def test_rejects_layer0_upward_dependency_including_target_and_renamed(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            _create_workspace(
                root,
                [
                    "crates/org-proto",
                    "crates/org-platform",
                    "crates/org-store",
                    "crates/org-command",
                    "crates/org-docs",
                    "crates/org-office",
                ],
                {
                    "crates/org-proto": (
                        '[package]\nname = "org-proto"\nversion = "0.0.0"\n'
                        '[dependencies]\norg-office = { path = "../org-office" }\n',
                        {"src/lib.rs": ""},
                    ),
                    "crates/org-platform": (
                        '[package]\nname = "org-platform"\nversion = "0.0.0"\n',
                        {"src/lib.rs": ""},
                    ),
                    "crates/org-store": (
                        '[package]\nname = "org-store"\nversion = "0.0.0"\n'
                        '[dependencies]\norg-platform = { path = "../org-platform" }\n'
                        '[target.\'cfg(unix)\'.dependencies]\noff = { package = "org-office", path = "../org-office" }\n',
                        {"src/lib.rs": ""},
                    ),
                    "crates/org-command": (
                        '[package]\nname = "org-command"\nversion = "0.0.0"\n'
                        '[dependencies]\norg-proto = { path = "../org-proto" }\n'
                        'org-docs = { path = "../org-docs" }\n',
                        {"src/lib.rs": ""},
                    ),
                    "crates/org-docs": (
                        '[package]\nname = "org-docs"\nversion = "0.0.0"\n',
                        {"src/lib.rs": ""},
                    ),
                    "crates/org-office": (
                        '[package]\nname = "org-office"\nversion = "0.0.0"\n'
                        '[dependencies]\norg-proto = { path = "../org-proto" }\n',
                        {"src/lib.rs": ""},
                    ),
                },
            )
            res = health.run_health_check(root)
            self.assertTrue(any("`org-proto`" in e and "`org-office`" in e for e in res.errors))
            self.assertTrue(any("`org-store`" in e and "`org-office`" in e for e in res.errors))

            # Remove upward edges: Layer-0 -> Layer-0 (org-store -> org-platform,
            # org-command -> org-proto + org-docs) passes cleanly.
            _write(
                root,
                "crates/org-proto/Cargo.toml",
                '[package]\nname = "org-proto"\nversion = "0.0.0"\n',
            )
            _write(
                root,
                "crates/org-store/Cargo.toml",
                '[package]\nname = "org-store"\nversion = "0.0.0"\n'
                '[dependencies]\norg-platform = { path = "../org-platform" }\n',
            )
            ok = health.run_health_check(root)
            self.assertEqual(ok.errors, [])

    def test_build_dependencies_cycle_and_layer0_upward_build_dep_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            _create_workspace(
                root,
                ["crates/org-proto", "crates/org-store", "crates/org-office"],
                {
                    "crates/org-proto": (
                        '[package]\nname = "org-proto"\nversion = "0.0.0"\n'
                        '[build-dependencies]\norg-office = { path = "../org-office" }\n',
                        {"src/lib.rs": ""},
                    ),
                    "crates/org-store": (
                        '[package]\nname = "org-store"\nversion = "0.0.0"\n'
                        '[target.\'cfg(unix)\'.build-dependencies]\n'
                        'off = { package = "org-office", path = "../org-office" }\n',
                        {"src/lib.rs": ""},
                    ),
                    "crates/org-office": (
                        '[package]\nname = "org-office"\nversion = "0.0.0"\n'
                        '[dependencies]\norg-proto = { path = "../org-proto" }\n',
                        {"src/lib.rs": ""},
                    ),
                },
            )
            res = health.run_health_check(root)
            self.assertTrue(
                any(
                    "Cargo DAG cycle" in e and "org-office -> org-proto -> org-office" in e
                    for e in res.errors
                ),
                f"expected build-dependencies cycle, got: {res.errors}",
            )
            self.assertTrue(
                any("Layer-0" in e and "`org-proto`" in e and "`org-office`" in e for e in res.errors),
                f"expected org-proto Layer-0 build-dep error, got: {res.errors}",
            )
            self.assertTrue(
                any("Layer-0" in e and "`org-store`" in e and "`org-office`" in e for e in res.errors),
                f"expected org-store target build-dep error, got: {res.errors}",
            )

    def test_renamed_workspace_dependencies_layer0_upward_dep_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            _create_workspace(
                root,
                ["crates/org-proto", "crates/org-store", "crates/org-office"],
                {
                    "crates/org-proto": (
                        '[package]\nname = "org-proto"\nversion = "0.0.0"\n'
                        "[dependencies]\noff = { workspace = true }\n",
                        {"src/lib.rs": ""},
                    ),
                    "crates/org-store": (
                        '[package]\nname = "org-store"\nversion = "0.0.0"\n'
                        "[build-dependencies]\noff.workspace = true\n",
                        {"src/lib.rs": ""},
                    ),
                    "crates/org-office": (
                        '[package]\nname = "org-office"\nversion = "0.0.0"\n',
                        {"src/lib.rs": ""},
                    ),
                },
                root_extra=(
                    "[workspace.dependencies]\n"
                    'off = { package = "org-office", path = "crates/org-office" }\n'
                ),
            )
            res = health.run_health_check(root)
            self.assertTrue(
                any("Layer-0" in e and "`org-proto`" in e and "`org-office`" in e for e in res.errors),
                f"expected renamed workspace dep violation on org-proto, got: {res.errors}",
            )
            self.assertTrue(
                any("Layer-0" in e and "`org-store`" in e and "`org-office`" in e for e in res.errors),
                f"expected renamed workspace build-dep violation on org-store, got: {res.errors}",
            )

    def test_layer0_includes_redact_sync_proto_host_control_and_office_launch(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            _create_workspace(
                root,
                [
                    "crates/org-proto",
                    "crates/org-redact",
                    "crates/org-sync-proto",
                    "crates/org-host-control",
                    "crates/org-office-launch",
                    "crates/org-host",
                ],
                {
                    "crates/org-proto": (
                        '[package]\nname = "org-proto"\nversion = "0.0.0"\n',
                        {"src/lib.rs": ""},
                    ),
                    "crates/org-redact": (
                        '[package]\nname = "org-redact"\nversion = "0.0.0"\n'
                        '[dependencies]\norg-host = { path = "../org-host" }\n',
                        {"src/lib.rs": ""},
                    ),
                    "crates/org-sync-proto": (
                        '[package]\nname = "org-sync-proto"\nversion = "0.0.0"\n'
                        '[dependencies]\norg-host = { path = "../org-host" }\n',
                        {"src/lib.rs": ""},
                    ),
                    "crates/org-host-control": (
                        '[package]\nname = "org-host-control"\nversion = "0.0.0"\n'
                        '[dependencies]\norg-proto = { path = "../org-proto" }\n'
                        'org-host = { path = "../org-host" }\n',
                        {"src/lib.rs": ""},
                    ),
                    "crates/org-office-launch": (
                        '[package]\nname = "org-office-launch"\nversion = "0.0.0"\n'
                        '[dependencies]\norg-proto = { path = "../org-proto" }\n'
                        'org-host = { path = "../org-host" }\n',
                        {"src/lib.rs": ""},
                    ),
                    "crates/org-host": (
                        '[package]\nname = "org-host"\nversion = "0.0.0"\n',
                        {"src/lib.rs": ""},
                    ),
                },
            )
            res = health.run_health_check(root)
            for crate in ("org-redact", "org-sync-proto", "org-host-control", "org-office-launch"):
                self.assertTrue(
                    any("Layer-0" in e and f"`{crate}`" in e and "`org-host`" in e for e in res.errors),
                    f"expected Layer-0 violation for {crate}, got: {res.errors}",
                )


class TestTomlFallbackAndCli(unittest.TestCase):
    def test_toml_fallback_parses_workspace_and_dependencies(self) -> None:
        sample = """\
[workspace]
members = [
  "crates/org-proto",
  "crates/org-store", # comment
]

[package]
name = "org-store"

[dependencies]
org-platform = { path = "../org-platform" }
renamed = { package = "org-proto", path = "../org-proto" }

[target.'cfg(unix)'.dependencies]
org-docs = { path = "../org-docs" }
"""
        parsed = health._parse_toml_fallback(sample)
        self.assertEqual(
            parsed["workspace"]["members"],
            ["crates/org-proto", "crates/org-store"],
        )
        self.assertEqual(parsed["package"]["name"], "org-store")
        self.assertEqual(parsed["dependencies"]["renamed"]["package"], "org-proto")
        self.assertIn("org-docs", parsed["target"]["cfg(unix)"]["dependencies"])

    def test_toml_fallback_array_of_tables_does_not_overwrite_package_name(self) -> None:
        sample = """\
[package]
name = "org-office-launch"
version = "0.0.0"

[[bin]]
name = "launch"
path = "src/main.rs"

[[test]]
name = "launch"
path = "tests/launch.rs"

[dependencies]
org-proto = { path = "../org-proto" }

[[example]]
name = "demo"
path = "examples/demo.rs"
"""
        parsed = health._parse_toml_fallback(sample)
        self.assertEqual(parsed["package"]["name"], "org-office-launch")
        self.assertNotIn("path", parsed["package"])
        self.assertEqual(list(parsed["dependencies"].keys()), ["org-proto"])

    def test_cli_exit_codes_and_print_baseline(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            _create_workspace(
                root,
                ["crates/org-proto"],
                {
                    "crates/org-proto": (
                        '[package]\nname = "org-proto"\nversion = "0.0.0"\n',
                        {"src/lib.rs": "pub fn ok() {}\n"},
                    )
                },
            )
            out = io.StringIO()
            err = io.StringIO()
            with redirect_stdout(out), redirect_stderr(err):
                code = health.main(["--repo-root", str(root)])
            self.assertEqual(code, 0)
            self.assertIn("Architecture health OK", out.getvalue())

            out2 = io.StringIO()
            with redirect_stdout(out2):
                code2 = health.main(["--repo-root", str(root), "--print-baseline"])
            self.assertEqual(code2, 0)
            self.assertIn("[crate_loc]", out2.getvalue())


if __name__ == "__main__":
    unittest.main()
