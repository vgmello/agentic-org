// Flow definition: Markdown blocks wired by transitions (REQ-01), each block owned by an
// agent so crossing to another agent's block is a handoff (REQ-02). Conditions are CEL.
// Pure: no I/O and no run state, so the UI, the runner and the agent CLI share it.

import { Environment } from '@marcbachmann/cel-js';

export interface AgentDef {
	label: string;
	persona: string;
}

export interface BlockDef {
	id: string;
	title: string;
	file: string;
	agent: string;
	stage: string;
	interactive?: boolean;
	produces?: string[];
}

/** How a transition fires. `on` absent = level-triggered: fires as soon as `when` holds. */
export interface TransitionRule {
	/** Event name: an agent outcome (`completed`) or a host event (`task.status_changed`). */
	on?: string;
	/** CEL filter over the event payload (`event`). */
	where?: string;
	/** CEL guard over facts (`flags`, `task`, `project`, ...). */
	when?: string;
}

export interface TransitionDef extends TransitionRule {
	from: string;
	to: string;
	/** Used instead when the host lacks a capability the rule references. */
	fallback?: TransitionRule;
}

export interface FlowDef {
	id: string;
	version: number;
	title: string;
	/** Judgments an agent may pass with an outcome, e.g. `standards_changed: bool`. */
	flags?: Record<string, string>;
	agents: Record<string, AgentDef>;
	triggers: { command: string; start: string }[];
	blocks: BlockDef[];
	transitions: TransitionDef[];
}

/**
 * A capability is an optional host integration that supplies facts under one CEL variable
 * and may emit host events. The flow engine owns these contracts; a host implements them.
 */
export interface CapabilityContract {
	name: string;
	schema: Record<string, string>;
	events: string[];
}

export const CAPABILITY_CATALOG: CapabilityContract[] = [
	{
		name: 'task',
		schema: { id: 'string', status: 'string' },
		events: ['task.status_changed']
	},
	{
		name: 'project',
		schema: { standards_defined: 'bool' },
		events: []
	}
];

/** A transition after capability resolution: its own rule or its fallback. */
export interface Transition extends TransitionRule {
	from: string;
	to: string;
	mode: 'native' | 'fallback';
	/** Capability this transition uses (native) or lacked (fallback). */
	capability?: string;
}

export interface Activation {
	block: BlockDef;
	agent: AgentDef & { id: string };
	markdown: string;
}

export type Facts = Record<string, unknown>;
export interface FlowEvent {
	type: string;
	payload: Record<string, unknown>;
}

export interface Flow {
	def: FlowDef;
	capabilities: string[];
	transitions: Transition[];
	activate(blockId: string): Activation;
	startBlock(command: string): string;
	outgoing(blockId: string): Transition[];
	/** Outcome names an agent may report from a block (host events excluded). */
	outcomes(blockId: string): string[];
	/** Flags an agent may pass with `outcome` from a block: those its transitions read. */
	flagsFor(blockId: string, outcome: string): string[];
	/** Transitions that fire for `event`, or level transitions when `event` is null. */
	match(blockId: string, event: FlowEvent | null, facts: Facts): Transition[];
}

export const BLOCK_FILE = /^blocks\/[\w-]+\.md$/;
const FLAG_NAME = /^[a-z][a-z0-9_]*$/;
const HOST_EVENTS = new Set(CAPABILITY_CATALOG.flatMap((c) => c.events));

function environment(flags: Record<string, string>, capabilities: CapabilityContract[]): Environment {
	const env = new Environment();
	env.registerVariable({ name: 'flags', schema: flags });
	env.registerVariable('event', 'map');
	for (const c of capabilities) env.registerVariable({ name: c.name, schema: c.schema });
	return env;
}

/**
 * Validates the wiring, type-checks every CEL expression against the host's capabilities and
 * binds block Markdown. A rule needing a capability the host lacks is replaced by its fallback.
 */
export function loadFlow(def: FlowDef, markdownByFile: Record<string, string>, provided: string[]): Flow {
	const blocks = new Map(def.blocks.map((b) => [b.id, b]));
	const errors: string[] = [];
	const flags = def.flags ?? {};
	const hostCaps = CAPABILITY_CATALOG.filter((c) => provided.includes(c.name));
	const hostEnv = environment(flags, hostCaps);

	for (const p of provided)
		if (!CAPABILITY_CATALOG.some((c) => c.name === p)) errors.push(`unknown capability ${p}`);
	for (const f of Object.keys(flags)) if (!FLAG_NAME.test(f)) errors.push(`flag ${f}: use snake_case`);

	const seen = (kind: string, keys: string[]) =>
		keys.forEach((k, i) => keys.indexOf(k) !== i && errors.push(`duplicate ${kind} ${k}`));
	seen('block', def.blocks.map((b) => b.id));
	seen('command', def.triggers.map((t) => t.command));

	for (const b of def.blocks) {
		if (!BLOCK_FILE.test(b.file)) errors.push(`block ${b.id}: file must be blocks/<name>.md`);
		if (!Object.hasOwn(def.agents, b.agent)) errors.push(`block ${b.id}: unknown agent ${b.agent}`);
		if (markdownByFile[b.file] === undefined) errors.push(`block ${b.id}: missing file ${b.file}`);
	}
	for (const t of def.triggers) {
		if (!blocks.has(t.start)) errors.push(`trigger ${t.command}: unknown block ${t.start}`);
	}

	const validWith = (rule: TransitionRule, caps: CapabilityContract[], env: Environment) =>
		(!rule.on || !HOST_EVENTS.has(rule.on) || caps.some((c) => c.events.includes(rule.on!))) &&
		[rule.where, rule.when].every((e) => !e || env.check(e).valid);

	/** null when valid for this host; else the missing capability's name, or 'invalid'. */
	const missingCapability = (rule: TransitionRule): string | null => {
		if (validWith(rule, hostCaps, hostEnv)) return null;
		for (const cap of CAPABILITY_CATALOG.filter((c) => !provided.includes(c.name))) {
			const caps = [...hostCaps, cap];
			if (validWith(rule, caps, environment(flags, caps))) return cap.name;
		}
		return 'invalid';
	};

	const usedCapability = (rule: TransitionRule) =>
		hostCaps.find((c) => c.events.includes(rule.on ?? '') || [rule.where, rule.when].some((e) => e?.includes(`${c.name}.`)))
			?.name;

	const describe = (rule: TransitionRule) =>
		[rule.on && `on ${rule.on}`, rule.where && `where ${rule.where}`, rule.when && `when ${rule.when}`]
			.filter(Boolean)
			.join(' ');

	const transitions: Transition[] = [];
	for (const t of def.transitions) {
		const label = `transition ${t.from}->${t.to}`;
		if (!blocks.has(t.from)) errors.push(`${label}: unknown from`);
		if (!blocks.has(t.to)) errors.push(`${label}: unknown to`);
		const shapeOk = (rule: TransitionRule, what: string) => {
			if (!rule.on && !rule.when) errors.push(`${label}: ${what} needs 'on' or a level 'when'`);
			if (rule.where && !rule.on) errors.push(`${label}: ${what} 'where' needs an 'on' event`);
		};
		shapeOk(t, 'rule');
		const missing = missingCapability(t);
		if (missing === null) {
			transitions.push({ from: t.from, to: t.to, on: t.on, where: t.where, when: t.when, mode: 'native', capability: usedCapability(t) });
		} else if (missing === 'invalid') {
			errors.push(`${label}: invalid condition (${describe(t)})`);
		} else if (!t.fallback) {
			errors.push(`${label}: needs capability ${missing} and has no fallback`);
		} else if (missingCapability(t.fallback) !== null) {
			errors.push(`${label}: fallback is not valid for this host (${describe(t.fallback)})`);
		} else {
			shapeOk(t.fallback, 'fallback');
			transitions.push({ from: t.from, to: t.to, ...t.fallback, mode: 'fallback', capability: missing });
		}
	}

	const outgoing = (blockId: string) => transitions.filter((t) => t.from === blockId);
	for (const b of def.blocks) {
		const unconditional = outgoing(b.id).filter((t) => !t.when && !t.where);
		for (const [on, group] of Map.groupBy(unconditional, (t) => t.on ?? ''))
			if (group.length > 1) errors.push(`block ${b.id}: ${group.length} unconditional transitions on ${on}`);
	}
	if (errors.length) throw new Error(`Invalid flow ${def.id}:\n${errors.join('\n')}`);

	const activate = (blockId: string): Activation => {
		const block = blocks.get(blockId);
		if (!block) throw new Error(`Unknown block ${blockId}`);
		return { block, agent: { id: block.agent, ...def.agents[block.agent] }, markdown: markdownByFile[block.file] };
	};

	const passes = (expr: string | undefined, ctx: Facts) => !expr || hostEnv.evaluate(expr, ctx) === true;

	return {
		def,
		capabilities: provided,
		transitions,
		activate,
		outgoing,
		startBlock(command) {
			const trigger = def.triggers.find((t) => t.command === command);
			if (!trigger) throw new Error(`Unknown command ${command}`);
			return trigger.start;
		},
		outcomes: (blockId) =>
			[...new Set(outgoing(blockId).flatMap((t) => (t.on && !HOST_EVENTS.has(t.on) ? [t.on] : [])))],
		flagsFor: (blockId, outcome) => [
			...new Set(
				outgoing(blockId)
					.filter((t) => t.on === outcome)
					.flatMap((t) => [...(t.when ?? '').matchAll(/\bflags\.([a-z][a-z0-9_]*)/g)].map((m) => m[1]))
			)
		],
		match(blockId, event, facts) {
			const ctx: Facts = { flags: {}, ...facts, event: event?.payload ?? {} };
			return outgoing(blockId).filter(
				(t) => (event ? t.on === event.type : !t.on) && passes(t.where, ctx) && passes(t.when, ctx)
			);
		}
	};
}
