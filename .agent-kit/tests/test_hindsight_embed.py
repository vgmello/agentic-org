"""Verify fail-closed daemon ownership/profile checks without changing live state."""
import json
from pathlib import Path
import sys
import shutil
import tempfile
import unittest
from unittest.mock import Mock, patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'runtime'))
import kit_hindsight_embed as embed


class DaemonPolicyTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.repo = Path(self.directory.name)
        policy_dir = self.repo / '.agent-kit/policy/hindsight'
        policy_dir.mkdir(parents=True)
        source = Path(__file__).resolve().parents[1] / 'policy/hindsight'
        for name in ('runtime.json', 'embed.json'):
            (policy_dir / name).write_bytes((source / name).read_bytes())
        self.policy = embed.settings(self.repo)
        self.runtime = json.loads((policy_dir / 'runtime.json').read_text())
        self.env, _ = embed.environment(self.policy, self.runtime, 'a' * 64)

    def verify_with(self, profile=None, process=None):
        manager = Mock()
        manager.profile_exists.return_value = True
        manager.load_profile_config.return_value = profile or {
            **self.env, 'HINDSIGHT_API_PORT': '9077'}
        if process is None:
            process = Mock(pid=123)
            process.environ.return_value = self.env
            process.net_connections.return_value = [Mock(status='LISTEN', laddr=Mock(port=9077, ip='127.0.0.1'))]
        with patch('hindsight_embed.profile_manager.ProfileManager', return_value=manager), \
             patch.object(embed, 'listener', return_value=process), \
             patch('hindsight_embed.daemon_embed_manager.DaemonEmbedManager') as daemon:
            value = embed.local(self.repo, 'verify', 'a' * 64, self.runtime)
            daemon.assert_not_called()
            manager.create_profile.assert_not_called()
            return value

    def test_current_local_codex_profile_is_accepted_without_starting(self):
        self.assertEqual(self.verify_with()['service_llm_provider'], 'openai-codex')

    def test_changed_saved_provider_is_refused(self):
        with self.assertRaisesRegex(ValueError, 'Existing daemon profile differs'):
            self.verify_with(profile={**self.env, 'HINDSIGHT_API_PORT': '9077',
                                      'HINDSIGHT_API_LLM_PROVIDER': 'anthropic'})

    def test_changed_running_provider_is_refused_even_when_file_matches(self):
        process = Mock()
        process.environ.return_value = {**self.env, 'HINDSIGHT_API_LLM_PROVIDER': 'anthropic'}
        with self.assertRaisesRegex(ValueError, 'Running daemon provider/profile drift'):
            self.verify_with(process=process)
        process.net_connections.assert_not_called()

    def test_nonloopback_binding_is_refused(self):
        process = Mock()
        process.environ.return_value = self.env
        process.net_connections.return_value = [Mock(status='LISTEN', laddr=Mock(port=9077, ip='0.0.0.0'))]
        with self.assertRaisesRegex(ValueError, 'exclusively on IPv4 loopback'):
            self.verify_with(process=process)

    def test_remote_endpoint_is_refused_before_process_access(self):
        with patch.object(embed, 'invoke') as invoke:
            with self.assertRaisesRegex(ValueError, 'approved local daemon'):
                embed.verify(self.repo, {'endpoint': 'http://example.com:9077'}, 'a' * 64, self.runtime)
            invoke.assert_not_called()

    def test_foreign_port_owner_is_preserved(self):
        process = Mock()
        process.uids.return_value.real = -1
        with patch.object(embed.subprocess, 'run', return_value=Mock(stdout='123\n')), \
             patch('psutil.Process', return_value=process):
            with self.assertRaisesRegex(ValueError, 'Another process owns'):
                embed.listener(self.policy)
            process.terminate.assert_not_called()
            process.kill.assert_not_called()


class ReviewIsolationTests(unittest.TestCase):
    def test_native_memory_is_disabled_for_review_repair_and_final_children(self):
        import uuid
        import kit_codex
        for phase in ('review', 'fix', 'final'):
            with self.subTest(phase=phase), tempfile.TemporaryDirectory() as directory:
                repo = Path(directory).resolve()
                session = str(uuid.uuid4())
                def launch(args, **kwargs):
                    def communicate(prompt, timeout):
                        result = Path(args[args.index('--output-last-message') + 1])
                        result.write_text(json.dumps({'verdict': 'clean', 'summary': 'fixture', 'findings': []}))
                        for event in ({'type': 'thread.started', 'thread_id': session},
                                      {'type': 'turn.completed', 'usage': {}}):
                            kwargs['stdout'].write(json.dumps(event) + '\n')
                        kwargs['stdout'].flush()
                    process = Mock(returncode=0)
                    process.communicate.side_effect = communicate
                    return process
                # The runner kills the turn's process group afterwards; the fake process has no real group.
                with patch.object(kit_codex.subprocess, 'Popen', side_effect=launch) as popen, \
                        patch.object(kit_codex.os, 'killpg'):
                    kit_codex.invoke(repo, repo / 'run', phase, 'Read source independently', 'high', 10,
                                     'gpt-6.2-sol', session=session if phase == 'fix' else None)
                args = popen.call_args.args[0]
                self.assertEqual(args[args.index('-m') + 1], 'gpt-6.2-sol')
                overrides = [args[i + 1] for i, value in enumerate(args[:-1]) if value == '-c']
                self.assertIn('features.hooks=false', overrides)
                self.assertIn('mcp_servers.hindsight.enabled=false', overrides)
                self.assertNotIn('mcp_servers.starter_kit_context.enabled=false', overrides)
                self.assertEqual(popen.call_args.kwargs['env']['HINDSIGHT_DISABLED'], 'true')
                self.assertEqual(popen.call_args.kwargs['env']['AGENT_KIT_MEMORY_ACCESS'], 'review')

    def test_fresh_review_runs_in_a_disposable_clone(self):
        import uuid
        import kit_codex
        with tempfile.TemporaryDirectory() as directory:
            repo = Path(directory).resolve() / 'repo'
            (repo / 'src').mkdir(parents=True)
            (repo / 'src/app.ts').write_text('source')
            (repo / 'target').mkdir()
            (repo / '.agent-kit/reviews').mkdir(parents=True)
            seen = {}
            real_popen = kit_codex.subprocess.Popen
            def launch(args, **kwargs):
                if args[0] != 'codex':  # cp/chflags/chmod used to make and remove the clone
                    return real_popen(args, **kwargs)
                seen.update(cwd=Path(kwargs['cwd']), tmp=Path(kwargs['env']['TMPDIR']))
                seen['copied'] = (seen['cwd'] / 'src/app.ts').read_text()
                seen['excluded'] = (seen['cwd'] / 'target').exists() or (seen['cwd'] / '.agent-kit/reviews').exists()
                def communicate(prompt, timeout):
                    Path(args[args.index('--output-last-message') + 1]).write_text(
                        json.dumps({'verdict': 'clean', 'summary': 'fixture', 'findings': []}))
                    for event in ({'type': 'thread.started', 'thread_id': str(uuid.uuid4())}, {'type': 'turn.completed', 'usage': {}}):
                        kwargs['stdout'].write(json.dumps(event) + '\n')
                    kwargs['stdout'].flush()
                process = Mock(returncode=0)
                process.communicate.side_effect = communicate
                return process
            box = {'mode': 'clone', 'network_access': True, 'exclude': ['target', '.agent-kit/reviews', '.agent-kit/logs']}
            with patch.object(kit_codex.subprocess, 'Popen', side_effect=launch) as popen, patch.object(kit_codex.os, 'killpg'):
                kit_codex.invoke(repo, repo / '.agent-kit/reviews/run', 'review', 'Review only', 'high', 10, 'gpt-6.2-sol', review_box=box)
            args = next(c.args[0] for c in popen.call_args_list if c.args[0][0] == 'codex')
            overrides = [args[i + 1] for i, value in enumerate(args[:-1]) if value == '-c']
            self.assertIn('sandbox_mode="workspace-write"', overrides)
            self.assertIn('sandbox_workspace_write.exclude_slash_tmp=true', overrides)
            self.assertIn('sandbox_workspace_write.exclude_tmpdir_env_var=true', overrides)
            self.assertIn(f'sandbox_workspace_write.writable_roots={json.dumps([str(seen["tmp"])])}', overrides)
            self.assertIn('--ignore-user-config', args)
            self.assertNotIn('mcp_servers.hindsight.enabled=false', overrides, 'that table is not loaded in a clone turn')
            self.assertNotEqual(seen['cwd'], repo)
            self.assertEqual(seen['copied'], 'source')
            self.assertFalse(seen['excluded'])
            self.assertFalse(seen['cwd'].exists(), 'the clone is deleted after the turn')
            self.assertFalse(seen['tmp'].exists(), 'the private temp folder is deleted after the turn')

    def test_review_sandbox_policy_comes_from_the_base_commit(self):
        import subprocess
        import kit_codex
        with tempfile.TemporaryDirectory() as directory:
            repo = Path(directory).resolve()
            git = lambda *a: subprocess.run(['git', '-C', str(repo), *a], check=True, capture_output=True, text=True).stdout.strip()
            git('init', '-q'); git('config', 'user.email', 't@example.com'); git('config', 'user.name', 't')
            (repo / 'f').write_text('x'); git('add', '.'); git('commit', '-qm', 'base')
            self.assertIsNone(kit_codex.review_sandbox(repo, git('rev-parse', 'HEAD')), 'no policy keeps read-only review')
            policy = repo / kit_codex.REVIEW_SANDBOX
            policy.parent.mkdir(parents=True)
            cases = {'{': 'Invalid', json.dumps({'version': 2, 'mode': 'clone', 'network_access': True, 'exclude': ['.']}): 'relative',
                     json.dumps({'version': 2, 'mode': 'clone', 'network_access': True, 'exclude': ['../x']}): 'relative',
                     json.dumps({'version': 1, 'writable_roots': []}): 'Invalid'}
            for text, message in cases.items():
                policy.write_text(text); git('add', '.'); git('commit', '-qm', 'policy')
                with self.subTest(text=text), self.assertRaisesRegex(ValueError, message):
                    kit_codex.review_sandbox(repo, git('rev-parse', 'HEAD'))
            for bad, message in ((dict(sandbox='root'), 'sandbox must be'),
                                 (dict(sandbox='danger-full-access', network_access=False), 'always has network'),
                                 (dict(sandbox='danger-full-access'), 'needs protected_refs'),
                                 (dict(protected_refs=['main']), 'refs/... patterns')):
                policy.write_text(json.dumps({'version': 2, 'mode': 'clone', 'network_access': True, 'exclude': [], **bad}))
                git('add', '.'); git('commit', '-qm', 'bad sandbox')
                with self.subTest(bad=bad), self.assertRaisesRegex(ValueError, message):
                    kit_codex.review_sandbox(repo, git('rev-parse', 'HEAD'))
            policy.write_text(json.dumps({'version': 2, 'mode': 'clone', 'network_access': False, 'exclude': ['target']}))
            git('add', '.'); git('commit', '-qm', 'good')
            box = kit_codex.review_sandbox(repo, git('rev-parse', 'HEAD'))
            self.assertEqual(box['sandbox'], 'workspace-write', 'the sandbox defaults to workspace-write')
            self.assertEqual(box['exclude'], sorted({'target', *kit_codex.CLONE_ALWAYS_EXCLUDED}))
            policy.write_text(json.dumps({'version': 2, 'mode': 'clone', 'network_access': True, 'exclude': []}))
            self.assertFalse(kit_codex.review_sandbox(repo, git('rev-parse', 'HEAD'))['network_access'],
                             'the working tree cannot change the pinned policy')

    def test_failed_clone_leaves_nothing_behind(self):
        import kit_codex
        with tempfile.TemporaryDirectory() as directory:
            repo = Path(directory).resolve() / 'repo'
            (repo / 'src').mkdir(parents=True)
            made = []
            real_mkdtemp = kit_codex.tempfile.mkdtemp
            def mkdtemp(**kwargs):
                made.append(Path(real_mkdtemp(**kwargs)))
                return str(made[-1])
            box = {'mode': 'clone', 'network_access': False, 'exclude': ['.agent-kit/reviews']}
            with patch.object(kit_codex.tempfile, 'mkdtemp', side_effect=mkdtemp), \
                    patch.object(kit_codex, 'clone_checkout', side_effect=OSError('disk full')), \
                    self.assertRaises(OSError):
                kit_codex.invoke(repo, repo / '.agent-kit/reviews/run', 'review', 'Review only', 'high', 10, 'gpt-6.2-sol', review_box=box)
            self.assertFalse((repo / '.agent-kit/reviews/run/review-clone').exists())
            self.assertTrue(made and not made[0].exists(), 'the private temp folder is removed')

    def test_repair_turn_is_pinned_to_the_real_checkout(self):
        import uuid
        import kit_codex
        with tempfile.TemporaryDirectory() as directory:
            repo = Path(directory).resolve()
            session = str(uuid.uuid4())
            def launch(args, **kwargs):
                def communicate(prompt, timeout):
                    Path(args[args.index('--output-last-message') + 1]).write_text(json.dumps({'findings': []}))
                    for event in ({'type': 'thread.started', 'thread_id': session}, {'type': 'turn.completed', 'usage': {}}):
                        kwargs['stdout'].write(json.dumps(event) + '\n')
                    kwargs['stdout'].flush()
                process = Mock(returncode=0)
                process.communicate.side_effect = communicate
                return process
            with patch.object(kit_codex.subprocess, 'Popen', side_effect=launch) as popen, patch.object(kit_codex.os, 'killpg'):
                kit_codex.invoke(repo, repo / 'run', 'fix', 'Fix', 'high', 10, 'gpt-6.2-sol', session=session)
            args = popen.call_args.args[0]
            self.assertEqual(args[2:5], ['-C', str(repo), 'resume'])
            self.assertEqual(Path(popen.call_args.kwargs['cwd']), repo)

    def test_full_access_review_keeps_the_clone_and_drops_workspace_flags(self):
        import subprocess
        import uuid
        import kit_codex
        with tempfile.TemporaryDirectory() as directory:
            repo = Path(directory).resolve() / 'repo'
            (repo / 'src').mkdir(parents=True)
            real_popen = kit_codex.subprocess.Popen
            seen = {}
            def launch(args, **kwargs):
                if args[0] != 'codex':
                    return real_popen(args, **kwargs)
                seen['cwd'] = Path(kwargs['cwd'])
                def communicate(prompt, timeout):
                    seen['prompt'] = prompt
                    Path(args[args.index('--output-last-message') + 1]).write_text(
                        json.dumps({'verdict': 'clean', 'summary': 'fixture', 'findings': []}))
                    for event in ({'type': 'thread.started', 'thread_id': str(uuid.uuid4())}, {'type': 'turn.completed', 'usage': {}}):
                        kwargs['stdout'].write(json.dumps(event) + '\n')
                    kwargs['stdout'].flush()
                process = Mock(returncode=0)
                process.communicate.side_effect = communicate
                return process
            subprocess.run(['git', 'init', '-q', str(repo)], check=True)
            box = {'mode': 'clone', 'sandbox': 'danger-full-access', 'network_access': True, 'exclude': ['.agent-kit/reviews']}
            with patch.object(kit_codex.subprocess, 'Popen', side_effect=launch) as popen, patch.object(kit_codex.os, 'killpg'):
                kit_codex.invoke(repo, repo / '.agent-kit/reviews/run', 'review', 'Review only', 'high', 10, 'gpt-6.2-sol', review_box=box)
            tamper = {'hook': True}
            def tampering(args, **kwargs):
                if args[0] == 'codex' and tamper.pop('hook', False):
                    (repo / '.git/hooks/pre-commit').write_text('#!/bin/sh\necho planted\n')
                return launch(args, **kwargs)
            with patch.object(kit_codex.subprocess, 'Popen', side_effect=tampering), patch.object(kit_codex.os, 'killpg'), \
                    self.assertRaisesRegex(ValueError, 'Git state the runner guards changed'):
                kit_codex.invoke(repo, repo / '.agent-kit/reviews/run2', 'review', 'Review only', 'high', 10, 'gpt-6.2-sol', review_box=box)
            def stalls(args, **kwargs):  # plants a hook, then times out
                if args[0] != 'codex':
                    return real_popen(args, **kwargs)
                (repo / '.git/hooks/post-commit').write_text('#!/bin/sh\necho planted\n')
                process = Mock(returncode=None)
                process.communicate.side_effect = subprocess.TimeoutExpired('codex', 10)
                return process
            with patch.object(kit_codex.subprocess, 'Popen', side_effect=stalls), patch.object(kit_codex.os, 'killpg'), \
                    self.assertRaisesRegex(ValueError, 'Git state the runner guards changed') as caught:
                kit_codex.invoke(repo, repo / '.agent-kit/reviews/run3', 'final', 'Review only', 'high', 10, 'gpt-6.2-sol', review_box=box)
            self.assertIn('timed out', str(caught.exception.__context__), 'the timeout stays attached')
            args = next(c.args[0] for c in popen.call_args_list if c.args[0][0] == 'codex')
            overrides = [args[i + 1] for i, value in enumerate(args[:-1]) if value == '-c']
            self.assertIn('sandbox_mode="danger-full-access"', overrides)
            self.assertIn('--skip-git-repo-check', args)
            self.assertFalse([o for o in overrides if o.startswith('sandbox_workspace_write.')])
            self.assertIn('--ignore-user-config', args)
            self.assertNotEqual(seen['cwd'], repo)
            self.assertFalse(seen['cwd'].exists(), 'the clone is still deleted')
            self.assertIn('worktree links or protected branches, tags and replace refs change', seen['prompt'])

    def test_git_guard_covers_what_git_runs_and_protected_refs_but_not_concurrent_work(self):
        import subprocess
        import kit_codex
        protected = ['refs/heads/integration', 'refs/tags/', 'refs/replace/']
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory).resolve()
            repo = base / 'repo'
            def git(*args, at=repo):
                subprocess.run(['git', '-C', str(at), '-c', 'user.name=t', '-c', 'user.email=t@t', *args], check=True,
                               capture_output=True)
            git('init', '-q', at=base)
            subprocess.run(['git', 'init', '-q', str(repo)], check=True)
            git('commit', '-q', '--allow-empty', '-m', 'base')
            git('branch', 'integration')
            for name in ('wt', 'other'):
                git('worktree', 'add', '-q', str(base / name))
            def append(path, text):
                with path.open('a') as handle:
                    handle.write(text)
            def changed(at, change):
                before = kit_codex.git_guard(at, protected)
                self.assertEqual([], kit_codex.guard_changed(before, kit_codex.git_guard(at, protected)), 'stable')
                change()
                return kit_codex.guard_changed(before, kit_codex.git_guard(at, protected))
            (repo / '.git/hooks/pre-push').write_text('#!/bin/sh\n')
            (repo / '.git/hooks/pre-push').chmod(0o644)
            blocking = {
                'config': lambda: append(repo / '.git/config', '[core]\n\tfsmonitor = x\n'),
                'info': lambda: append(repo / '.git/info/exclude', 'hidden\n'),
                'common config.worktree': lambda: (repo / '.git/config.worktree').write_text('[core]\n'),
                'own config.worktree': lambda: (repo / '.git/worktrees/wt/config.worktree').write_text('[core]\n'),
                "another worktree's pointer": lambda: append(repo / '.git/worktrees/other/commondir', '\n'),
                'alternates': lambda: (repo / '.git/objects/info/alternates').write_text('/elsewhere\n'),
                'protected branch moved': lambda: git('branch', '-f', 'integration', 'wt'),
                'tag': lambda: git('tag', 'v-moved'),
                'shallow': lambda: (repo / '.git/shallow').write_text('0' * 40 + '\n'),
                'protected branch tracking': lambda: git('branch', '--set-upstream-to', 'wt', 'integration'),
                'own info': lambda: ((repo / '.git/worktrees/wt/info').mkdir(exist_ok=True),
                                     (repo / '.git/worktrees/wt/info/sparse-checkout').write_text('/x\n')),
                'hook made executable': lambda: (repo / '.git/hooks/pre-push').chmod(0o755),
                'hooks replaced by a link': lambda: (shutil.move(repo / '.git/hooks', base / 'hooks'),
                                                     (repo / '.git/hooks').symlink_to(base / 'hooks')),
            }
            git('commit', '-q', '--allow-empty', '-m', 'x', at=base / 'wt')
            for name, change in blocking.items():
                with self.subTest(change=name):
                    self.assertTrue(changed(base / 'wt', change))
            # Reviewing the main checkout guards the same things.
            self.assertTrue(changed(repo, lambda: append(repo / '.git/config', '[core]\n\tpager = x\n')))
            git('commit', '-q', '--allow-empty', '-m', 'z', at=base / 'other')
            self.assertTrue(changed(repo, lambda: git('branch', '-f', 'integration', 'other')))
            # Ordinary concurrent work elsewhere does not block a review.
            quiet = {
                'commit in another worktree': lambda: git('commit', '-q', '--allow-empty', '-m', 'y', at=base / 'other'),
                'switch branches elsewhere': lambda: git('switch', '-q', '-c', 'feature', at=base / 'other'),
                'new branch': lambda: git('branch', 'scratch'),
                'delete branch': lambda: git('branch', '-D', 'scratch'),
                'stash': lambda: ((base / 'other/f').write_text('x'), git('add', 'f', at=base / 'other'),
                                  git('stash', '-q', at=base / 'other')),
                'pack refs': lambda: git('pack-refs', '--all'),
                'branch with upstream elsewhere': lambda: git('switch', '-q', '-c', 'tracked', '--track', 'origin/main',
                                                              at=base / 'other'),
                'push -u style tracking': lambda: git('branch', '--set-upstream-to', 'origin/main', 'scratch2'),
                'delete a tracked branch': lambda: git('branch', '-D', 'scratch2'),
                'add a worktree': lambda: git('worktree', 'add', '-q', str(base / 'third')),
                'remove a worktree': lambda: git('worktree', 'remove', str(base / 'third')),
            }
            # A remote to track: the origin's main is the base commit.
            git('remote', 'add', 'origin', str(repo))
            git('fetch', '-q', 'origin', 'main:refs/remotes/origin/main')
            git('branch', 'scratch2')
            for name, change in quiet.items():
                with self.subTest(quiet=name):
                    self.assertEqual([], changed(base / 'wt', change))

    @unittest.skipUnless(sys.platform == 'darwin', 'BSD file flags')
    def test_remove_clone_clears_locks_a_reviewer_can_set(self):
        import subprocess
        import kit_codex
        with tempfile.TemporaryDirectory() as directory:
            clone = Path(directory) / 'clone'
            (clone / 'locked/dir').mkdir(parents=True)
            (clone / 'locked/file').write_text('x')
            subprocess.run(['chmod', '+a', 'everyone deny delete', str(clone / 'locked/file')], check=True)
            for flag, target in (('uchg', 'locked/file'), ('uappnd', 'locked/dir'), ('uappnd', 'locked')):
                subprocess.run(['chflags', flag, str(clone / target)], check=True)
            outside = Path(directory) / 'outside'
            outside.write_text('real')
            (clone / 'link').symlink_to(outside)
            kit_codex.remove_clone(clone)
            self.assertFalse(clone.exists())
            self.assertEqual(outside.read_text(), 'real', 'links are removed, never followed')

    def test_sweep_kills_processes_left_working_in_the_folders(self):
        import subprocess
        import time
        import kit_codex
        if not kit_codex.shutil.which('lsof'):
            self.skipTest('lsof unavailable')
        folder = Path(tempfile.mkdtemp(prefix='sweep-'))  # unresolved, e.g. /var/... on macOS
        stray = bystander = None
        try:
            stray = subprocess.Popen(['sleep', '60'], cwd=folder, start_new_session=True)
            bystander = subprocess.Popen(['sleep', '60'], start_new_session=True)
            time.sleep(0.3)
            kit_codex.sweep([folder])
            self.assertIsNotNone(stray.wait(timeout=10), 'a process working in the folder is killed')
            self.assertIsNone(bystander.poll(), 'other processes are left alone')
        finally:
            for process in (stray, bystander):
                if process is not None:
                    process.kill()
                    process.wait()
            kit_codex.remove_clone(folder)

    def test_documented_plan_review_command_is_memory_isolated(self):
        import os
        import subprocess
        recipe = (Path(__file__).resolve().parents[2] / '.agents/skills/plan-review/SKILL.md').read_text()
        command = recipe.split('```bash\n', 1)[1].split('```', 1)[0]
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            output = root / '.agent-kit/logs/plan-review'
            output.mkdir(parents=True)
            (output / 'prompt.txt').write_text('Review this design independently')
            binary = root / 'bin'
            binary.mkdir()
            executable = binary / 'codex'
            executable.write_text('#!' + str(Path(sys.executable).resolve()) + '\n'
                                  'import os,sys,json\n'
                                  'print(json.dumps({"args":sys.argv[1:],"disabled":os.environ.get("HINDSIGHT_DISABLED"),'
                                  '"phase":os.environ.get("AGENT_KIT_MEMORY_ACCESS")}))\n')
            executable.chmod(0o700)
            subprocess.run(['bash', '-c', command], cwd=root,
                           env={**os.environ, 'PATH': str(binary) + os.pathsep + os.environ['PATH']}, check=True)
            captured = json.loads((output / 'events.jsonl').read_text())
        args = captured['args']
        overrides = [args[i + 1] for i, arg in enumerate(args[:-1]) if arg == '-c']
        self.assertEqual(captured['disabled'], 'true')
        self.assertEqual(captured['phase'], 'review')
        self.assertIn('features.hooks=false', overrides)
        self.assertIn('mcp_servers.hindsight.enabled=false', overrides)
        self.assertIn('read-only', args)


if __name__ == '__main__':
    unittest.main()
