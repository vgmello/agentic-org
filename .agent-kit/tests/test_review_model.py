"""The Sol review model comes from the routing policy and is checked against a fixed list."""
import json
from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'runtime'))
import kit_codex
import kit_policy

POLICY = Path(__file__).resolve().parents[1] / 'policy/model-routing.json'


class ReviewModelPolicyTests(unittest.TestCase):
    def load_with(self, model):
        value = json.loads(POLICY.read_text())
        value['review']['model'] = model
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'model-routing.json'
            path.write_text(json.dumps(value))
            return kit_policy.load_policy(path)

    def test_each_supported_sol_model_is_accepted(self):
        for model in kit_policy.SOL_REVIEW_MODELS:
            with self.subTest(model=model):
                self.load_with(model)

    def test_any_other_review_model_is_refused(self):
        for model in ('opus', 'gpt-6.2-sol; x', 'gpt-6.3-sol', ''):
            with self.subTest(model=model), self.assertRaises(ValueError):
                self.load_with(model)


class RunReviewModelTests(unittest.TestCase):
    def test_a_run_uses_its_recorded_model(self):
        self.assertEqual(kit_codex.review_model({'review_model': 'gpt-6.2-sol'}), 'gpt-6.2-sol')

    def test_a_run_recorded_before_the_policy_named_the_model_ran_on_6_1(self):
        self.assertEqual(kit_codex.review_model({}), 'gpt-6.1-sol')


class RunPolicyMatchTests(unittest.TestCase):
    def test_a_run_recorded_on_another_review_model_is_refused(self):
        value = json.loads(POLICY.read_text())
        value['review']['model'] = 'gpt-6.2-sol'
        for recorded in ({'review_model': 'gpt-6.1-sol'}, {}):
            with self.subTest(recorded=recorded), tempfile.TemporaryDirectory() as directory:
                repo = Path(directory).resolve()
                policy = repo / '.agent-kit/policy/model-routing.json'
                policy.parent.mkdir(parents=True)
                policy.write_text(json.dumps(value))
                _, checksum = kit_policy.load_policy(policy)
                run = repo / '.agent-kit/reviews/run'
                run.mkdir(parents=True)
                (run / 'run.json').write_text(json.dumps({
                    'repo': str(repo), 'mode': value['mode'], 'routing': {'policy_sha256': checksum},
                    'final_model': value['review']['final_model'], **recorded}))
                with self.assertRaisesRegex(ValueError, 'Run models differ'):
                    kit_codex.current_run(run)


class FinalModelTests(unittest.TestCase):
    """Non-regression: the codex-only final review still accepts only the policy's final model."""

    def state(self):
        return {'phase': 'AWAITING_FINAL', 'mode': 'codex-only', 'final_model': 'gpt-6.1-sol',
                'final_effort': 'high', 'head': 'a' * 40, 'origin_session': 'origin', 'rounds': []}

    def result(self, model):
        return {'head': 'a' * 40, 'session_id': 'fresh', 'model': model, 'effort': 'high',
                'review': {'verdict': 'clean', 'summary': 'fixture', 'findings': []}}

    def test_codex_only_final_accepts_the_recorded_final_model(self):
        with tempfile.TemporaryDirectory() as directory:
            state = self.state()
            kit_codex.record_final(Path(directory).resolve(), state, self.result('gpt-6.1-sol'))
            self.assertEqual(state['phase'], 'ACCEPTED')

    def test_codex_only_final_accepts_higher_effort_including_max_and_refuses_downgrade(self):
        with tempfile.TemporaryDirectory() as directory:
            state = {**self.state(), 'final_model': 'gpt-6.2-sol', 'final_effort': 'xhigh'}
            result = {**self.result('gpt-6.2-sol'), 'effort': 'max'}
            kit_codex.record_final(Path(directory).resolve(), state, result)
            self.assertEqual(state['phase'], 'ACCEPTED')
        with tempfile.TemporaryDirectory() as directory:
            state = {**self.state(), 'final_model': 'gpt-6.2-sol', 'final_effort': 'xhigh'}
            result = {**self.result('gpt-6.2-sol'), 'effort': 'high'}
            with self.assertRaisesRegex(ValueError, 'effort'):
                kit_codex.record_final(Path(directory).resolve(), state, result)

    def test_codex_only_final_refuses_any_other_model(self):
        for model in ('gpt-6.2-sol', 'claude-opus-5-5'):
            with self.subTest(model=model), tempfile.TemporaryDirectory() as directory:
                with self.assertRaisesRegex(ValueError, 'profile model'):
                    kit_codex.record_final(Path(directory).resolve(), self.state(), self.result(model))


if __name__ == '__main__':
    unittest.main()
