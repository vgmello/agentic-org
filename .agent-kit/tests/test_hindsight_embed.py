"""Verify fail-closed daemon ownership/profile checks without changing live state."""
import json
from pathlib import Path
import sys
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
                                     session=session if phase == 'fix' else None)
                args = popen.call_args.args[0]
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
                kit_codex.invoke(repo, repo / '.agent-kit/reviews/run', 'review', 'Review only', 'high', 10, review_box=box)
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
            policy.write_text(json.dumps({'version': 2, 'mode': 'clone', 'network_access': False, 'exclude': ['target']}))
            git('add', '.'); git('commit', '-qm', 'good')
            box = kit_codex.review_sandbox(repo, git('rev-parse', 'HEAD'))
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
                kit_codex.invoke(repo, repo / '.agent-kit/reviews/run', 'review', 'Review only', 'high', 10, review_box=box)
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
                kit_codex.invoke(repo, repo / 'run', 'fix', 'Fix', 'high', 10, session=session)
            args = popen.call_args.args[0]
            self.assertEqual(args[2:5], ['-C', str(repo), 'resume'])
            self.assertEqual(Path(popen.call_args.kwargs['cwd']), repo)

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
