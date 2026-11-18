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
                with patch.object(kit_codex.subprocess, 'Popen', side_effect=launch) as popen:
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
