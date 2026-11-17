import type { Flow } from './engine';
import { emptyHost, factSource, moveTask, type HostState, type TaskStatus } from './host';
import { Runner, emptyState, type LogEntry, type RunRecord, type RunState } from './runtime';
import { makeFlow } from './workflow';

export const SUBJECT_TASK = 'T-1';

/**
 * UI simulation: the runner plus fake host integrations. The runner mutates its own plain
 * state; after each action the view copies a snapshot into Svelte state.
 */
class Simulation {
	capabilities = $state<string[]>(['task', 'project']);
	flow = $state.raw<Flow>(makeFlow(['task', 'project']));
	host = $state<HostState>(emptyHost());
	view = $state.raw<RunState>(emptyState());
	loseNextDispatch = $state(false);
	lastResult = $state<string | null>(null);
	restarts = $state(0);

	private runner!: Runner;

	run = $derived<RunRecord | undefined>(this.view.runs.at(-1));
	step = $derived(this.run?.steps.at(-1));
	log = $derived<LogEntry[]>([...this.view.log].reverse());

	constructor() {
		this.reset(['task', 'project']);
	}

	reset(capabilities: string[]) {
		this.capabilities = capabilities;
		this.flow = makeFlow(capabilities);
		this.host = emptyHost();
		this.lastResult = null;
		this.restarts = 0;
		this.boot(emptyState());
	}

	/** Simulates an office restart: a new runner from the saved snapshot only. */
	restart() {
		this.restarts++;
		this.boot(this.runner.snapshot());
		this.runner.reconcile();
		this.sync();
	}

	start(command: string) {
		this.act(() => this.runner.start(command, { task: SUBJECT_TASK }) && null);
	}

	begin() {
		if (this.step) this.act(() => (this.runner.begin(this.step!.id), null));
	}

	report(outcome: string, flags: Record<string, boolean>) {
		const step = this.step;
		if (step) this.act(() => this.runner.report(`${step.id}:${outcome}:${crypto.randomUUID()}`, step.id, outcome, flags));
	}

	moveTask(to: TaskStatus) {
		// Without the task capability the runner never hears this; a tick still re-checks facts.
		this.act(() => (moveTask(this.host, this.runner, SUBJECT_TASK, to), null));
	}

	setStandards(value: boolean) {
		this.act(() => ((this.host.project.standards_defined = value), this.runner.reconcile(), null));
	}

	tick() {
		this.act(() => (this.runner.tick(), null));
	}

	private boot(state: RunState) {
		// The host state object is shared with the fact source; Svelte proxies are fine here.
		this.runner = new Runner(this.flow, state, factSource(this.host, this.capabilities), () => {
			if (this.loseNextDispatch) {
				this.loseNextDispatch = false;
				throw new Error('agent host unreachable (simulated)');
			}
		});
		this.sync();
	}

	private act(fn: () => string | null) {
		try {
			this.lastResult = fn();
		} catch (e) {
			this.lastResult = `error: ${(e as Error).message}`;
		}
		this.sync();
	}

	private sync() {
		this.view = this.runner.snapshot();
	}
}

export const sim = new Simulation();
