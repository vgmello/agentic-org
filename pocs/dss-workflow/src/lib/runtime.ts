// Flow runner: owns run state and is the only thing that moves a run. Agents and the host
// only report (outcomes, host events); the runner matches them to a run's current step,
// commits the move, and its reconcile loop makes sure the next step's agent gets started.
// State is a plain serializable snapshot, standing in for the office journal.

import type { Activation, Facts, Flow, FlowEvent, Transition } from './engine';

export type StepState = 'ready' | 'dispatched' | 'running' | 'done';

export interface StepRecord {
	id: string;
	block: string;
	agent: string;
	state: StepState;
	attempts: number;
	openedAt: number;
	dispatchedAt?: number;
	/** After a failed dispatch: not retried before this clock value (backoff). */
	retryAt?: number;
	startedAt?: number;
	closedAt?: number;
	closedBy?: string;
}

export interface Subject {
	task?: string;
}

export interface RunRecord {
	id: string;
	flow: string;
	flowVersion: number;
	subject: Subject;
	flags: Record<string, boolean>;
	status: 'active' | 'needs_attention';
	attention?: string;
	steps: StepRecord[];
}

export interface LogEntry {
	at: number;
	run?: string;
	text: string;
}

export interface RunState {
	clock: number;
	seq: number;
	runs: RunRecord[];
	/** Operation id → result, so a retried report is answered, not re-applied. */
	receipts: Record<string, string>;
	log: LogEntry[];
}

/** Host facts for a run's subject, one key per provided capability (e.g. `task`). */
export type FactSource = (run: RunRecord) => Facts;

/** Starts (or wakes) the step's agent with the block Markdown. May throw: the loop retries. */
export type Dispatcher = (run: RunRecord, step: StepRecord, activation: Activation) => void;

export const DISPATCH_LEASE = 3;
export const MAX_ATTEMPTS = 3;
const MAX_LEVEL_MOVES = 20;

export const emptyState = (): RunState => ({ clock: 0, seq: 0, runs: [], receipts: {}, log: [] });

export class Runner {
	constructor(
		readonly flow: Flow,
		private state: RunState,
		private readonly facts: FactSource,
		private readonly dispatcher: Dispatcher
	) {}

	snapshot(): RunState {
		return structuredClone(this.state);
	}

	get runs(): readonly RunRecord[] {
		return this.state.runs;
	}

	get log(): readonly LogEntry[] {
		return this.state.log;
	}

	get clock(): number {
		return this.state.clock;
	}

	current(run: RunRecord): StepRecord {
		return run.steps[run.steps.length - 1];
	}

	start(command: string, subject: Subject = {}): RunRecord {
		const block = this.flow.startBlock(command);
		const run: RunRecord = {
			id: `R${++this.state.seq}`,
			flow: this.flow.def.id,
			flowVersion: this.flow.def.version,
			subject,
			flags: {},
			status: 'active',
			steps: []
		};
		this.state.runs.push(run);
		this.open(run, block);
		this.note(run, `started by /${command}${subject.task ? ` for ${subject.task}` : ''}`);
		this.reconcile();
		return run;
	}

	/** The step's agent acknowledges its turn started (a real host: the turn is running). */
	begin(stepId: string): void {
		const { step } = this.find(stepId);
		if (step.state !== 'dispatched') throw new Error(`step ${stepId} is ${step.state}, not dispatched`);
		step.state = 'running';
		step.startedAt = this.state.clock;
	}

	/** An agent reports its block's outcome. Exactly correlated by step id; idempotent by op id. */
	report(opId: string, stepId: string, outcome: string, flags: Record<string, boolean> = {}): string {
		const replay = this.state.receipts[opId];
		if (replay !== undefined) return replay;
		const result = this.applyReport(stepId, outcome, flags);
		// Only accepted reports are receipted, so a rejected one can be retried once the run allows it.
		if (result.startsWith('accepted')) this.state.receipts[opId] = result;
		this.reconcile();
		return result;
	}

	/** A host event (e.g. `task.status_changed`), correlated to runs by subject. */
	deliver(event: FlowEvent, subject: Subject): string[] {
		const related = this.state.runs.filter((r) => r.status === 'active' && !!subject.task && r.subject.task === subject.task);
		const before = new Map(related.map((r) => [r.id, r.steps.length]));
		for (const run of related) {
			const step = this.current(run);
			const t = this.choose(run, this.flow.match(step.block, event, this.factsFor(run)), event.type);
			if (t) this.advance(run, t, `${event.type} ${JSON.stringify(event.payload)}`);
		}
		// The event may also have changed facts that level-triggered transitions read.
		this.reconcile();
		const moved = related.filter((r) => r.steps.length !== before.get(r.id)).map((r) => r.id);
		if (!moved.length) this.note(undefined, `${event.type} ${JSON.stringify(event.payload)} moved no run (recorded, ignored)`);
		return moved;
	}

	/** One loop pass: level-triggered moves, then make sure every open step's agent is working. */
	reconcile(): void {
		for (const run of this.state.runs) {
			if (run.status !== 'active') continue;
			for (let i = 0; i < MAX_LEVEL_MOVES; i++) {
				const step = this.current(run);
				const t = this.choose(run, this.flow.match(step.block, null, this.factsFor(run)), 'level');
				if (!t) break;
				this.advance(run, t, `level: ${t.when}`);
			}
			if (run.status === 'active') this.ensureDispatched(run);
		}
	}

	/** Time passes (a real host: a timer); expired dispatches are retried. */
	tick(): void {
		this.state.clock++;
		this.reconcile();
	}

	private applyReport(stepId: string, outcome: string, flags: Record<string, boolean>): string {
		const { run, step } = this.find(stepId);
		if (run.status !== 'active') return `rejected: run ${run.id} needs attention`;
		if (step !== this.current(run) || step.state === 'done') return `rejected: step ${stepId} is no longer current`;
		if (step.state !== 'running') return `rejected: step ${stepId} has not started`;
		if (!this.flow.outcomes(step.block).includes(outcome))
			return `rejected: ${outcome} is not an outcome of ${step.block}`;
		const allowed = this.flow.flagsFor(step.block, outcome);
		const extra = Object.keys(flags).filter((f) => !allowed.includes(f));
		if (extra.length) return `rejected: flag(s) ${extra.join(', ')} not used by ${outcome}; accepted: ${allowed.join(', ') || 'none'}`;
		const scratch = { ...run, flags: { ...run.flags, ...flags } };
		const t = this.choose(run, this.flow.match(step.block, { type: outcome, payload: {} }, this.factsFor(scratch)), outcome);
		if (!t) return run.status === 'active' ? `rejected: no transition on ${outcome} matches the current facts` : `rejected: ${run.attention}`;
		run.flags = scratch.flags;
		this.advance(run, t, `${step.agent} reported ${outcome}`);
		return `accepted: ${step.block} -> ${t.to}`;
	}

	/** Exactly one transition may fire; more is ambiguous and stops the run for a person. */
	private choose(run: RunRecord, matches: Transition[], what: string): Transition | undefined {
		if (matches.length <= 1) return matches[0];
		run.status = 'needs_attention';
		run.attention = `${matches.length} transitions match ${what}: ${matches.map((t) => t.to).join(', ')}`;
		this.note(run, `needs attention: ${run.attention}`);
		return undefined;
	}

	/** One commit: close the current step and open the next. Dispatch happens later, in reconcile. */
	private advance(run: RunRecord, t: Transition, cause: string): void {
		const step = this.current(run);
		step.state = 'done';
		step.closedAt = this.state.clock;
		step.closedBy = cause;
		this.open(run, t.to);
		const next = this.current(run);
		const handoff = next.agent !== step.agent ? ` — handoff ${step.agent} → ${next.agent}` : '';
		this.note(run, `${step.block} → ${t.to} (${cause})${handoff}`);
	}

	private open(run: RunRecord, blockId: string): void {
		const block = this.flow.activate(blockId).block;
		run.steps.push({
			id: `${run.id}.${run.steps.length + 1}`,
			block: blockId,
			agent: block.agent,
			state: 'ready',
			attempts: 0,
			openedAt: this.state.clock
		});
	}

	private ensureDispatched(run: RunRecord): void {
		const step = this.current(run);
		const expired = step.state === 'dispatched' && this.state.clock - step.dispatchedAt! >= DISPATCH_LEASE;
		const due = step.state === 'ready' && (step.retryAt ?? 0) <= this.state.clock;
		if (!due && !expired) return;
		if (step.attempts >= MAX_ATTEMPTS) {
			run.status = 'needs_attention';
			run.attention = `${step.agent} did not start ${step.block} after ${step.attempts} attempts — escalated to the lead`;
			this.note(run, run.attention);
			return;
		}
		step.attempts++;
		try {
			this.dispatcher(run, step, this.flow.activate(step.block));
			step.state = 'dispatched';
			step.dispatchedAt = this.state.clock;
			step.retryAt = undefined;
			this.note(run, `dispatched ${step.block} to ${step.agent} (attempt ${step.attempts})`);
		} catch (e) {
			step.state = 'ready';
			step.retryAt = this.state.clock + 1;
			this.note(run, `dispatch of ${step.block} failed: ${(e as Error).message} — retry at t${step.retryAt}`);
		}
	}

	private factsFor(run: RunRecord): Facts {
		const flags = Object.fromEntries(Object.keys(this.flow.def.flags ?? {}).map((f) => [f, run.flags[f] === true]));
		return { ...this.facts(run), flags };
	}

	private find(stepId: string): { run: RunRecord; step: StepRecord } {
		for (const run of this.state.runs) {
			const step = run.steps.find((s) => s.id === stepId);
			if (step) return { run, step };
		}
		throw new Error(`unknown step ${stepId}`);
	}

	private note(run: RunRecord | undefined, text: string): void {
		this.state.log.push({ at: this.state.clock, run: run?.id, text });
	}
}
