// Flow engine: Markdown blocks wired by JSON (REQ-01), with status-driven
// handoff to a different agent (REQ-02). Pure: no I/O, so the UI and the
// agent CLI share it.

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

export interface Transition {
	from: string;
	on: string;
	to: string;
	/** Context flag guard: `flag` or `!flag`. */
	when?: string;
}

export interface FlowDef {
	id: string;
	version: number;
	title: string;
	agents: Record<string, AgentDef>;
	triggers: { command: string; start: string }[];
	blocks: BlockDef[];
	transitions: Transition[];
}

export type Context = Record<string, boolean>;

export interface Activation {
	block: BlockDef;
	agent: AgentDef & { id: string };
	markdown: string;
	/** What caused this activation: `command <name>` or `<from> on <event>`. */
	cause: string;
	/** True when the previous block belonged to another agent. */
	handoff: boolean;
}

export interface Flow {
	def: FlowDef;
	activate(blockId: string, cause: string, previousAgent?: string): Activation;
	runCommand(command: string): Activation;
	fire(fromBlockId: string, event: string, ctx: Context): Activation;
	outgoing(blockId: string): Transition[];
}

export const BLOCK_FILE = /^blocks\/[\w-]+\.md$/;
const GUARD = /^!?[\w.-]+$/;

export function guardPasses(when: string | undefined, ctx: Context): boolean {
	if (!when) return true;
	const flag = guardFlag(when);
	const set = Object.hasOwn(ctx, flag) && ctx[flag] === true;
	return when.startsWith('!') ? !set : set;
}

export function guardFlag(when: string): string {
	return when.startsWith('!') ? when.slice(1) : when;
}

/** Context flags referenced by transition guards. */
export function guardFlags(def: FlowDef): string[] {
	return [...new Set(def.transitions.flatMap((t) => (t.when ? [guardFlag(t.when)] : [])))];
}

/** Validates the wiring and binds block Markdown by file path. Throws on broken wiring. */
export function loadFlow(def: FlowDef, markdownByFile: Record<string, string>): Flow {
	const blocks = new Map(def.blocks.map((b) => [b.id, b]));
	const errors: string[] = [];

	const seen = (kind: string, keys: string[]) =>
		keys.forEach((k, i) => keys.indexOf(k) !== i && errors.push(`duplicate ${kind} ${k}`));
	seen('block', def.blocks.map((b) => b.id));
	seen('command', def.triggers.map((t) => t.command));
	seen('transition', def.transitions.map((t) => `${t.from} on ${t.on} when ${t.when ?? '-'}`));

	for (const b of def.blocks) {
		if (!BLOCK_FILE.test(b.file)) errors.push(`block ${b.id}: file must be blocks/<name>.md`);
		if (!Object.hasOwn(def.agents, b.agent)) errors.push(`block ${b.id}: unknown agent ${b.agent}`);
		if (markdownByFile[b.file] === undefined) errors.push(`block ${b.id}: missing file ${b.file}`);
	}
	for (const t of def.triggers) {
		if (!blocks.has(t.start)) errors.push(`trigger ${t.command}: unknown block ${t.start}`);
	}
	for (const t of def.transitions) {
		if (!blocks.has(t.from)) errors.push(`transition ${t.from}->${t.to}: unknown from`);
		if (!blocks.has(t.to)) errors.push(`transition ${t.from}->${t.to}: unknown to`);
		if (t.when !== undefined && !GUARD.test(t.when)) errors.push(`transition ${t.from}->${t.to}: bad guard ${t.when}`);
	}
	// Alternatives for one event must be exactly `flag` / `!flag`, so one always matches.
	const groups = Map.groupBy(def.transitions, (t) => `${t.from} on ${t.on}`);
	for (const [key, group] of groups) {
		if (group.length < 2) continue;
		const guards = group.map((t) => t.when ?? '').sort();
		const flag = guardFlag(guards[0]);
		if (guards.length !== 2 || !flag || guards[0] !== `!${flag}` || guards[1] !== flag)
			errors.push(`transition ${key}: alternatives must be guarded by one flag and its negation`);
	}
	if (errors.length) throw new Error(`Invalid flow ${def.id}:\n${errors.join('\n')}`);

	const activate = (blockId: string, cause: string, previousAgent?: string): Activation => {
		const block = blocks.get(blockId);
		if (!block) throw new Error(`Unknown block ${blockId}`);
		return {
			block,
			agent: { id: block.agent, ...def.agents[block.agent] },
			markdown: markdownByFile[block.file],
			cause,
			handoff: previousAgent !== undefined && previousAgent !== block.agent
		};
	};

	const outgoing = (blockId: string) => def.transitions.filter((t) => t.from === blockId);

	return {
		def,
		activate,
		outgoing,
		runCommand(command) {
			const trigger = def.triggers.find((t) => t.command === command);
			if (!trigger) throw new Error(`Unknown command ${command}`);
			return activate(trigger.start, `command ${command}`);
		},
		fire(fromBlockId, event, ctx) {
			const from = blocks.get(fromBlockId);
			if (!from) throw new Error(`Unknown block ${fromBlockId}`);
			const candidates = outgoing(fromBlockId).filter((t) => t.on === event);
			if (!candidates.length) throw new Error(`No transition from ${fromBlockId} on ${event}`);
			const match = candidates.find((t) => guardPasses(t.when, ctx));
			if (!match) throw new Error(`Guard blocked ${fromBlockId} on ${event} (needs ${candidates.map((t) => t.when).join(' or ')})`);
			return activate(match.to, `${fromBlockId} on ${event}`, from.agent);
		}
	};
}
