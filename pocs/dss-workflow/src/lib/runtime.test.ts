import { describe, expect, it } from 'vitest';
import { emptyHost, factSource, moveTask, type HostState } from './host';
import { readFlow } from './load-node';
import { DISPATCH_LEASE, MAX_ATTEMPTS, Runner, emptyState, type Dispatcher, type RunState } from './runtime';

const ok: Dispatcher = () => {};
const failing: Dispatcher = () => {
	throw new Error('agent host unreachable');
};

function setup(caps: string[], state: RunState = emptyState(), host: HostState = emptyHost(), dispatcher = ok) {
	return { host, runner: new Runner(readFlow(caps), state, factSource(host, caps), dispatcher) };
}

/** Start the current step's turn and report an outcome. */
function act(runner: Runner, run: { steps: { id: string }[] }, outcome: string, flags: Record<string, boolean> = {}) {
	const step = run.steps.at(-1)!;
	runner.begin(step.id);
	return runner.report(`op-${step.id}-${outcome}`, step.id, outcome, flags);
}

const block = (runner: Runner, id: string) => runner.current(runner.runs.find((r) => r.id === id)!).block;

function toExecute(runner: Runner) {
	const run = runner.start('dss', { task: 'T-1' });
	act(runner, run, 'context_sufficient');
	act(runner, run, 'published', runner.flow.capabilities.includes('project') ? {} : { standards_defined: true });
	act(runner, run, 'recorded');
	act(runner, run, 'tasks_created');
	expect(block(runner, run.id)).toBe('execute-tasks');
	return run;
}

describe('runner with task + project integration', () => {
	it('reads facts instead of trusting agent flags', () => {
		const { host, runner } = setup(['task', 'project']);
		host.project.standards_defined = true;
		const run = runner.start('dss', { task: 'T-1' });
		act(runner, run, 'context_sufficient');
		expect(act(runner, run, 'published', { standards_defined: false })).toMatch(/not used by published/);
		expect(runner.report('op-again', run.steps.at(-1)!.id, 'published')).toBe('accepted: publish-product-dss -> tech-dss-autonomous');
	});

	it('moves to review when the task is in review — level-triggered, even if the event is missed', () => {
		const { host, runner } = setup(['task', 'project']);
		const run = toExecute(runner);
		host.tasks['T-1'].status = 'in_review'; // changed behind the runner's back: no event
		expect(block(runner, run.id)).toBe('execute-tasks');
		runner.tick();
		expect(block(runner, run.id)).toBe('code-review');
		expect(run.steps.at(-1)!.agent).toBe('reviewer');
		expect(runner.log.some((l) => l.text.includes('handoff executor → reviewer'))).toBe(true);
	});

	it('filters edge events with where, and correlates them by subject', () => {
		const { host, runner } = setup(['task', 'project']);
		host.tasks['T-2'] = { status: 'todo' };
		const run = toExecute(runner);
		moveTask(host, runner, 'T-1', 'in_review');
		expect(block(runner, run.id)).toBe('code-review');
		moveTask(host, runner, 'T-2', 'in_progress'); // other task: ignored
		moveTask(host, runner, 'T-1', 'todo'); // where: event.to == 'in_progress' — no match
		expect(block(runner, run.id)).toBe('code-review');
		moveTask(host, runner, 'T-1', 'in_progress');
		expect(block(runner, run.id)).toBe('execute-tasks');
	});

	it('guards a judgment with a fact: approval needs the task done', () => {
		const { host, runner } = setup(['task', 'project']);
		const run = toExecute(runner);
		moveTask(host, runner, 'T-1', 'in_review');
		const step = run.steps.at(-1)!;
		runner.begin(step.id);
		expect(runner.report('a1', step.id, 'approved')).toMatch(/no transition on approved matches/);
		moveTask(host, runner, 'T-1', 'done');
		expect(runner.report('a2', step.id, 'approved')).toBe('accepted: code-review -> reconcile');
	});
});

describe('same flow, no integration', () => {
	it('runs on agent outcomes and flags only', () => {
		const { runner } = setup([]);
		const run = toExecute(runner);
		expect(act(runner, run, 'submitted_for_review')).toBe('accepted: execute-tasks -> code-review');
		expect(act(runner, run, 'changes_requested')).toBe('accepted: code-review -> execute-tasks');
		act(runner, run, 'submitted_for_review');
		expect(act(runner, run, 'approved')).toBe('accepted: code-review -> reconcile');
	});

	it('ignores task events it has no capability for', () => {
		const { host, runner } = setup([]);
		const run = toExecute(runner);
		moveTask(host, runner, 'T-1', 'in_review');
		runner.tick();
		expect(block(runner, run.id)).toBe('execute-tasks');
	});
});

describe('making sure the next step runs', () => {
	it('survives a crash between the move and the dispatch', () => {
		const host = emptyHost();
		const first = setup(['task', 'project'], emptyState(), host);
		const run = toExecute(first.runner);
		// The office dies right after committing the move: dispatch never happens.
		const crashed = new Runner(first.runner.flow, first.runner.snapshot(), factSource(host, ['task', 'project']), failing);
		moveTask(host, crashed, 'T-1', 'in_review');
		const saved = crashed.snapshot();
		expect(saved.runs[0].steps.at(-1)).toMatchObject({ block: 'code-review', state: 'ready' });
		// Restart from the journal: the loop sees an open step with no turn and starts it
		// once its retry backoff has passed.
		const restarted = setup(['task', 'project'], saved, host).runner;
		restarted.reconcile();
		expect(restarted.runs[0].steps.at(-1)!.state).toBe('ready');
		restarted.tick();
		expect(restarted.runs[0].steps.at(-1)).toMatchObject({ block: 'code-review', state: 'dispatched', agent: 'reviewer' });
		expect(run.id).toBe(restarted.runs[0].id);
	});

	it('logs an event as ignored only when no run moved', () => {
		const { host, runner } = setup(['task', 'project']);
		toExecute(runner);
		moveTask(host, runner, 'T-1', 'in_progress');
		expect(runner.log.at(-1)!.text).toMatch(/moved no run/);
		moveTask(host, runner, 'T-1', 'in_review'); // no edge rule, but the level rule moves it
		expect(runner.log.some((l) => /in_review.*moved no run/.test(l.text))).toBe(false);
	});

	it('re-dispatches when the agent never starts, then escalates', () => {
		const { runner } = setup(['task', 'project']);
		const run = runner.start('dss', { task: 'T-1' });
		for (let i = 0; i < DISPATCH_LEASE * MAX_ATTEMPTS; i++) runner.tick();
		expect(run.steps[0].attempts).toBe(MAX_ATTEMPTS);
		expect(run.status).toBe('needs_attention');
		expect(run.attention).toMatch(/escalated to the lead/);
	});

	it('rejects stale, early, unknown and host-only reports; replays retries', () => {
		const { runner } = setup([]);
		const run = runner.start('dss', { task: 'T-1' });
		const first = run.steps[0];
		expect(runner.report('x0', first.id, 'context_sufficient')).toMatch(/has not started/);
		runner.begin(first.id);
		expect(runner.report('x1', first.id, 'task.status_changed')).toMatch(/not an outcome/);
		expect(runner.report('x2', first.id, 'context_sufficient')).toMatch(/accepted/);
		expect(runner.report('x2', first.id, 'context_sufficient')).toMatch(/accepted/); // same op: replayed
		expect(run.steps).toHaveLength(2);
		expect(runner.report('x3', first.id, 'context_sufficient')).toMatch(/no longer current/);
	});

	it('retries a rejected report under the same op id once it is accepted', () => {
		const { runner } = setup([]);
		const run = runner.start('dss', { task: 'T-1' });
		const first = run.steps[0];
		expect(runner.report('r1', first.id, 'context_sufficient')).toMatch(/has not started/);
		runner.begin(first.id);
		expect(runner.report('r1', first.id, 'context_sufficient')).toMatch(/accepted/);
	});
});
