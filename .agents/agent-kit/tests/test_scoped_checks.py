"""Checks run only for what changed since a base; a non-blocking check's failure is a warning."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'runtime'))
import kit_config
import kit_evidence
import kit_hooks

PRINT_ARGS = 'import sys, pathlib; pathlib.Path("ran.txt").open("a").write(" ".join(sys.argv[1:]) + "\\n")'


def git(repo, *args):
    return subprocess.run(['git', '-C', str(repo), *args], check=True, capture_output=True, text=True).stdout.strip()


def record(label):
    return ['python3', '-c', PRINT_ARGS, label]


class ScopedCheckTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.repo = Path(self.directory.name).resolve()
        git(self.repo, 'init', '-q', '-b', 'main')
        git(self.repo, 'config', 'user.email', 'test@example.com')
        git(self.repo, 'config', 'user.name', 'test')
        (self.repo / '.gitignore').write_text('.agents/agent-kit/logs/\nran.txt\n')
        (self.repo / 'ui/src').mkdir(parents=True)
        (self.repo / 'ui/src/a.ts').write_text('a\n')
        (self.repo / 'ui/package.json').write_text('{}\n')
        (self.repo / 'crates/proto/vectors').mkdir(parents=True)
        (self.repo / 'crates/proto/vectors/v.json').write_text('{}\n')
        (self.repo / 'core.rs').write_text('fn main() {}\n')
        (self.repo / '.agents/agent-kit').mkdir(parents=True, exist_ok=True)
        self.config = self.repo / '.agents/agent-kit/verification.json'

    def configure(self, ui_checks, base=None):
        """Write and commit the configuration; the commit is the base for later changes."""
        config = {'version': 1, 'timeout_seconds': 60, 'projects': [
            {'path': '.', 'checks': [['python3', '-c', 'pass']]},
            {'path': 'ui', 'checks': ui_checks}]}
        if base:
            config['base'] = base
        self.config.write_text(json.dumps(config))
        git(self.repo, 'add', '-A')
        git(self.repo, 'commit', '-q', '-m', 'configure')
        self.base = git(self.repo, 'rev-parse', 'HEAD')

    def ran(self):
        path = self.repo / 'ui/ran.txt'
        return path.read_text().splitlines() if path.exists() else []

    def verify(self, base):
        return kit_hooks.verify(self.repo, self.config, base)

    def test_a_check_whose_paths_nothing_changed_under_is_skipped(self):
        self.configure([{'argv': record('ui'), 'paths': ['ui/src/*']}])
        (self.repo / 'core.rs').write_text('fn main() { }\n')
        completed, receipt, _ = self.verify(self.base)
        self.assertEqual(self.ran(), [])
        self.assertTrue(any(line.startswith('SKIP') for line in completed))
        self.assertTrue(receipt['passed'])
        self.assertEqual(receipt['scope']['base'], self.base)
        self.assertEqual(receipt['executed'], 1)

    def test_a_check_runs_when_a_changed_file_matches_its_paths(self):
        self.configure([{'argv': record('ui'), 'paths': ['ui/src/*']}])
        (self.repo / 'ui/src/a.ts').write_text('b\n')
        self.verify(self.base)
        self.assertEqual(self.ran(), ['ui'])

    def test_a_file_outside_the_project_can_trigger_its_check(self):
        self.configure([{'argv': record('ui'), 'paths': ['ui/*', 'crates/*/vectors/*']}])
        (self.repo / 'crates/proto/vectors/v.json').write_text('{"a": 1}\n')
        self.verify(self.base)
        self.assertEqual(self.ran(), ['ui'])

    def test_committed_untracked_and_deleted_changes_all_count(self):
        self.configure([{'argv': record('ui'), 'paths': ['ui/src/*']}])
        (self.repo / 'ui/src/new.ts').write_text('n\n')
        self.verify(self.base)
        self.assertEqual(self.ran(), ['ui'])
        (self.repo / 'ui/ran.txt').unlink()
        git(self.repo, 'add', '-A')
        git(self.repo, 'commit', '-q', '-m', 'change')
        self.verify(self.base)
        self.assertEqual(self.ran(), ['ui'])
        (self.repo / 'ui/ran.txt').unlink()
        git(self.repo, 'rm', '-q', 'ui/src/a.ts', 'ui/src/new.ts')
        git(self.repo, 'commit', '-q', '-m', 'delete')
        self.verify(git(self.repo, 'rev-parse', 'HEAD~1'))
        self.assertEqual(self.ran(), ['ui'])

    def test_without_a_base_every_check_runs(self):
        self.configure([{'argv': record('ui'), 'paths': ['ui/src/*']}])
        _, receipt, _ = self.verify(None)
        self.assertEqual(self.ran(), ['ui'])
        self.assertEqual(receipt['scope'], 'all')

    def test_a_changed_configuration_runs_every_check(self):
        self.configure([{'argv': record('full'), 'paths': ['ui/src/*']}, {'argv': record('other'), 'paths': ['nothing/*']}])
        config = json.loads(self.config.read_text())
        config['timeout_seconds'] = 59
        self.config.write_text(json.dumps(config))
        completed, receipt, _ = self.verify(self.base)
        self.assertEqual(self.ran(), ['full', 'other'])
        self.assertTrue(receipt['scope']['configuration_changed'])

    def test_non_blocking_checks_run_last_and_their_failure_is_a_warning(self):
        self.configure([{'argv': ['python3', '-c', 'raise SystemExit(1)'], 'paths': ['ui/src/*'], 'blocking': False},
                        {'argv': record('blocking'), 'paths': ['ui/src/*']}])
        (self.repo / 'ui/src/a.ts').write_text('b\n')
        completed, receipt, path = self.verify(self.base)
        self.assertTrue(receipt['passed'])
        self.assertEqual(self.ran(), ['blocking'])
        self.assertTrue(completed[-1].startswith('WARN'))
        self.assertTrue(receipt['checks'][-1]['advisory_failure'])
        self.assertEqual(len(kit_evidence.inspect_receipt(path)['warnings']), 1)

    def test_a_non_blocking_check_that_cannot_launch_fails_verification(self):
        self.configure([{'argv': ['./missing-tool'], 'paths': ['ui/src/*'], 'blocking': False}])
        (self.repo / 'ui/src/a.ts').write_text('b\n')
        with self.assertRaises(kit_hooks.VerificationError):
            self.verify(self.base)

    def test_a_blocking_failure_still_fails_verification(self):
        self.configure([{'argv': ['python3', '-c', 'raise SystemExit(1)'], 'paths': ['ui/src/*']}])
        (self.repo / 'ui/src/a.ts').write_text('b\n')
        with self.assertRaises(kit_hooks.VerificationError):
            self.verify(self.base)

    def test_an_unknown_base_is_refused_with_a_clear_reason(self):
        self.configure([['python3', '-c', 'pass']])
        with self.assertRaisesRegex(kit_hooks.VerificationError, 'Cannot scope checks to base'):
            self.verify('no-such-branch')

    def test_the_configured_base_is_the_default(self):
        self.configure([['python3', '-c', 'pass']], base='main')
        git(self.repo, 'switch', '-q', '-c', 'feature')
        self.assertEqual(kit_hooks.default_base(self.repo, self.config), 'main')

    def test_without_a_configured_base_or_upstream_there_is_no_default(self):
        self.configure([['python3', '-c', 'pass']])
        self.assertIsNone(kit_hooks.default_base(self.repo, self.config))

    def test_a_non_blocking_check_never_uses_the_time_kept_for_evidence(self):
        self.configure([{'argv': ['python3', '-c', 'import time; time.sleep(30)'], 'paths': ['ui/src/*'],
                         'blocking': False}])
        config = json.loads(self.config.read_text())
        config['timeout_seconds'] = 20
        self.config.write_text(json.dumps(config))
        git(self.repo, 'commit', '-qam', 'short budget')
        (self.repo / 'ui/src/a.ts').write_text('b\n')
        completed, receipt, _ = self.verify(git(self.repo, 'rev-parse', 'HEAD'))
        self.assertTrue(receipt['passed'])
        self.assertEqual(receipt['checks'][-1]['skipped'], 'time budget')
        self.assertTrue(completed[-1].startswith('WARN'))

    def test_an_upstream_is_the_default_only_when_it_is_another_branch(self):
        self.configure([['python3', '-c', 'pass']])
        remote = tempfile.TemporaryDirectory()
        self.addCleanup(remote.cleanup)
        git(Path(remote.name), 'init', '-q', '--bare')
        git(self.repo, 'remote', 'add', 'origin', remote.name)
        git(self.repo, 'push', '-q', 'origin', 'main')
        git(self.repo, 'switch', '-q', '-c', 'feature')
        git(self.repo, 'push', '-q', '-u', 'origin', 'feature')
        self.assertIsNone(kit_hooks.default_base(self.repo, self.config))
        git(self.repo, 'branch', '-q', '--set-upstream-to', 'origin/main')
        self.assertEqual(kit_hooks.default_base(self.repo, self.config), 'origin/main')

    def test_a_non_blocking_check_skipped_for_time_is_a_receipt_warning(self):
        self.configure([{'argv': ['python3', '-c', 'pass'], 'paths': ['ui/src/*'], 'blocking': False}])
        config = json.loads(self.config.read_text())
        config['timeout_seconds'] = 20
        self.config.write_text(json.dumps(config))
        git(self.repo, 'commit', '-qam', 'short budget')
        (self.repo / 'ui/src/a.ts').write_text('b\n')
        _, _, path = self.verify(git(self.repo, 'rev-parse', 'HEAD'))
        self.assertEqual(kit_evidence.inspect_receipt(path)['warnings'],
                         ['Non-blocking check skipped (no time left): python3 -c pass'])

    def test_on_the_base_branch_itself_every_check_runs(self):
        self.configure([['python3', '-c', 'pass']], base='origin/main')
        self.assertIsNone(kit_hooks.default_base(self.repo, self.config))
        git(self.repo, 'switch', '-q', '-c', 'feature')
        self.assertEqual(kit_hooks.default_base(self.repo, self.config), 'origin/main')

    def test_a_non_blocking_check_is_stopped_at_its_cap(self):
        self.configure([{'argv': ['python3', '-c', 'import time; time.sleep(30)'], 'paths': ['ui/src/*'],
                         'blocking': False}])
        (self.repo / 'ui/src/a.ts').write_text('b\n')
        original = kit_hooks.ADVISORY_SECONDS
        kit_hooks.ADVISORY_SECONDS = 1
        self.addCleanup(setattr, kit_hooks, 'ADVISORY_SECONDS', original)
        _, receipt, _ = self.verify(self.base)
        self.assertTrue(receipt['passed'])
        self.assertTrue(receipt['checks'][-1]['timed_out'])
        self.assertTrue(receipt['checks'][-1]['advisory_failure'])

    def test_evidence_reports_a_receipt_that_failed_before_scoping(self):
        self.configure([['python3', '-c', 'pass']])
        with self.assertRaises(kit_hooks.VerificationError) as failure:
            self.verify('no-such-branch')
        script = Path(__file__).resolve().parents[1] / 'runtime/kit_hooks.py'
        result = subprocess.run([sys.executable, str(script), 'evidence', '--receipt', str(failure.exception.receipt_path)],
                                capture_output=True, text=True)
        self.assertIn('STALE/FAILED', result.stdout)
        self.assertIn('Scope: unknown', result.stdout)

    def test_a_base_that_looks_like_an_option_is_refused(self):
        self.configure([['python3', '-c', 'pass']])
        with self.assertRaisesRegex(kit_hooks.VerificationError, 'is not a ref'):
            self.verify('--all')

    def test_a_project_outside_the_checkout_runs_in_full(self):
        outside = tempfile.TemporaryDirectory()
        self.addCleanup(outside.cleanup)
        other = Path(outside.name).resolve()
        (other / 'ran.txt').touch()
        self.configure([['python3', '-c', 'pass']])
        config = json.loads(self.config.read_text())
        config['projects'].append({'path': str(other), 'checks': [
            {'argv': ['python3', '-c', 'open("ran.txt", "a").write("other\\n")'], 'paths': ['ui/src/*']}]})
        self.config.write_text(json.dumps(config))
        git(self.repo, 'commit', '-qam', 'outside project')
        self.verify(git(self.repo, 'rev-parse', 'HEAD'))
        self.assertEqual((other / 'ran.txt').read_text(), 'other\n')

    def test_a_review_package_refuses_evidence_scoped_to_a_narrower_base(self):
        import kit_review
        (self.repo / '.gitignore').write_text('.agents/agent-kit/logs/\n.agents/agent-kit/reviews/\nran.txt\n')
        self.configure([{'argv': record('ui'), 'paths': ['ui/src/*']}])
        older = self.base
        (self.repo / 'ui/src/a.ts').write_text('b\n')
        git(self.repo, 'commit', '-qam', 'work')
        newer = git(self.repo, 'rev-parse', 'HEAD')
        _, _, narrow = self.verify(newer)
        with self.assertRaisesRegex(ValueError, 'covers less'):
            kit_review.package(self.repo, older, Path(narrow))
        _, _, wide = self.verify(older)
        _, _, everything = self.verify(None)
        for receipt in (wide, everything):
            with self.subTest(receipt=receipt):
                kit_review.package(self.repo, older, Path(receipt))
        config = json.loads(self.config.read_text())
        config['timeout_seconds'] = 59
        self.config.write_text(json.dumps(config))
        git(self.repo, 'commit', '-qam', 'config change')
        _, _, reconfigured = self.verify(git(self.repo, 'rev-parse', 'HEAD~1'))
        kit_review.package(self.repo, older, Path(reconfigured))

    def test_a_review_package_refuses_evidence_that_missed_a_file_reverted_on_the_branch(self):
        import kit_review
        (self.repo / '.gitignore').write_text('.agents/agent-kit/logs/\n.agents/agent-kit/reviews/\nran.txt\n')
        self.configure([{'argv': record('ui'), 'paths': ['ui/src/*']}])
        older = self.base
        (self.repo / 'ui/src/a.ts').write_text('changed on the base\n')
        git(self.repo, 'commit', '-qam', 'base changes a.ts')
        newer = git(self.repo, 'rev-parse', 'HEAD')
        git(self.repo, 'switch', '-q', '-c', 'feature')
        (self.repo / 'ui/src/a.ts').write_text('a\n')
        git(self.repo, 'commit', '-qam', 'feature reverts a.ts')
        _, receipt, path = self.verify(older)
        self.assertEqual(self.ran(), [])
        with self.assertRaisesRegex(ValueError, 'covers less'):
            kit_review.package(self.repo, newer, Path(path))

    def test_a_configured_base_that_is_not_text_is_refused_not_ignored(self):
        self.configure([['python3', '-c', 'pass']])
        config = json.loads(self.config.read_text())
        config['base'] = 5
        self.config.write_text(json.dumps(config))
        self.assertIsNone(kit_hooks.default_base(self.repo, self.config))
        with self.assertRaisesRegex(kit_hooks.VerificationError, 'base must be a Git ref'):
            self.verify(None)

    def test_an_older_receipt_without_a_scope_reads_as_all(self):
        self.configure([['python3', '-c', 'pass']])
        _, _, path = self.verify(None)
        receipt = json.loads(Path(path).read_text())
        del receipt['scope']
        Path(path).write_text(json.dumps(receipt))
        self.assertEqual(kit_evidence.inspect_receipt(path)['scope'], 'all')


class CommandLineTests(unittest.TestCase):
    def run_hooks(self, *args):
        script = Path(__file__).resolve().parents[1] / 'runtime/kit_hooks.py'
        return subprocess.run([sys.executable, str(script), *args], capture_output=True, text=True, input='{}')

    def test_base_and_all_cannot_be_combined(self):
        self.assertNotEqual(self.run_hooks('check', '--base', 'main', '--all').returncode, 0)

    def test_scope_flags_belong_to_the_standalone_check_only(self):
        for action in ('guard', 'format', 'verify'):
            with self.subTest(action=action):
                result = self.run_hooks(action, '--all')
                self.assertIn('only supported by the standalone check', result.stdout + result.stderr)


class CheckEntryTests(unittest.TestCase):
    def entry(self, value):
        return kit_config.check_entry(Path('/project'), value)

    def test_a_plain_argument_array_always_runs_and_blocks(self):
        check = self.entry(['cargo', 'test'])
        self.assertEqual((check.paths, check.blocking), (None, True))

    def test_invalid_entries_are_refused(self):
        for value in ({'argv': ['x'], 'paths': ['/abs']},
                      {'argv': ['x'], 'paths': ['../out/*']},
                      {'argv': ['x'], 'paths': ['./src/*']},
                      {'argv': ['x'], 'paths': ['src//a']},
                      {'argv': ['x'], 'paths': []},
                      {'argv': ['x'], 'blocking': 'no'},
                      {'argv': ['x'], 'unknown': 1},
                      {'argv': ['x'], 'related_argv': ['y', 'z'], 'paths': ['*']}):
            with self.subTest(value=value), self.assertRaises(kit_config.CheckError):
                self.entry(value)

    def test_covered_matches_workspace_relative_patterns(self):
        check = self.entry({'argv': ['x'], 'paths': ['project/src/*', 'project/package.json']})
        changed = [Path('/w/project/src/lib/a.ts'), Path('/w/project/package.json'),
                   Path('/w/project/README.md'), Path('/elsewhere/project/src/b.ts')]
        self.assertEqual(kit_config.covered(check, changed, Path('/w')),
                         [Path('/w/project/package.json'), Path('/w/project/src/lib/a.ts')])


if __name__ == '__main__':
    unittest.main()
