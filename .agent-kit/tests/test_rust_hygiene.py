import pathlib
import sys
import tempfile
import unittest
from unittest.mock import patch

RUNTIME = pathlib.Path(__file__).resolve().parents[1] / "runtime"
if str(RUNTIME) not in sys.path:
    sys.path.insert(0, str(RUNTIME))

import kit_rust_hygiene as hygiene


class RustWorkspaceLintsTests(unittest.TestCase):
    def test_manifest_inherits_workspace_lints_parses_tables_and_dotted_keys(self):
        self.assertTrue(
            hygiene.manifest_inherits_workspace_lints(
                '[package]\nname = "a"\n\n[lints]\nworkspace = true\n'
            )
        )
        self.assertTrue(
            hygiene.manifest_inherits_workspace_lints(
                'lints.workspace = true\n\n[package]\nname = "b"\n'
            )
        )
        self.assertFalse(
            hygiene.manifest_inherits_workspace_lints(
                '[package]\nname = "c"\ndescription = """\n[lints]\nworkspace = true\n"""\n'
            )
        )
        self.assertFalse(
            hygiene.manifest_inherits_workspace_lints(
                '[package]\nname = "d"\n\n[lints]\nworkspace = "true"\n'
            )
        )
        self.assertFalse(hygiene.manifest_inherits_workspace_lints('[package]\nname = "e"\n'))

    def test_check_workspace_lints_reports_missing_out_of_tree_and_missing_parser(self):
        with tempfile.TemporaryDirectory() as tmp:
            base = pathlib.Path(tmp)
            repo = base / "repo"
            (repo / "crates" / "ok").mkdir(parents=True)
            (repo / "crates" / "bad").mkdir(parents=True)
            ok_toml = repo / "crates" / "ok" / "Cargo.toml"
            bad_toml = repo / "crates" / "bad" / "Cargo.toml"
            outside_toml = base / "outside.toml"
            ok_toml.write_text('[package]\nname = "ok"\n\n[lints]\nworkspace = true\n', encoding="utf-8")
            bad_toml.write_text('[package]\nname = "bad"\n', encoding="utf-8")
            outside_toml.write_text('[package]\nname = "ext"\n\n[lints]\nworkspace = true\n', encoding="utf-8")

            with patch.object(
                hygiene,
                "workspace_member_manifests",
                return_value=[ok_toml, bad_toml, outside_toml],
            ):
                findings = hygiene.check_workspace_lints(repo)
            self.assertEqual(len(findings), 2, findings)
            self.assertTrue(any("crates/bad/Cargo.toml" in f and "missing-workspace-lints" in f for f in findings))
            self.assertTrue(any("unsafe-path" in f for f in findings))

            with patch.object(hygiene, "tomllib", None), patch.object(
                hygiene, "workspace_member_manifests"
            ) as meta_mock:
                parser_findings = hygiene.check_workspace_lints(repo)
                meta_mock.assert_not_called()
            self.assertTrue(any("unreadable-manifest" in f for f in parser_findings), parser_findings)

    def test_repository_workspace_members_all_inherit_workspace_lints(self):
        self.assertEqual(hygiene.check_workspace_lints(), [])


if __name__ == "__main__":
    unittest.main()
