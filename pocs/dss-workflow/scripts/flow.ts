// Agent-side entry point (REQ-01): an agent runs a command or reports an event
// and receives the Markdown of the block it must execute next.
//   bun scripts/flow.ts command project-setup
//   bun scripts/flow.ts fire publish-product-dss product-dss.published standards.defined
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { BLOCK_FILE, guardFlag, loadFlow, type Activation, type Flow, type FlowDef } from '../src/lib/engine.ts';

const USAGE = 'usage: flow.ts command <name> | fire <block> <event> [flag...]';
const root = join(import.meta.dirname, '..', 'workflow');

function load(): Flow {
	const def = JSON.parse(readFileSync(join(root, 'dss.flow.json'), 'utf8')) as FlowDef;
	// Read only blocks/<name>.md paths; loadFlow reports anything skipped as a missing/invalid file.
	const md: Record<string, string> = {};
	for (const b of def.blocks) {
		if (!BLOCK_FILE.test(b.file)) continue;
		try {
			md[b.file] = readFileSync(join(root, b.file), 'utf8');
		} catch {
			// missing file: reported by loadFlow
		}
	}
	return loadFlow(def, md);
}

function fail(message: string): never {
	console.error(`error: ${message}\n${USAGE}`);
	process.exit(2);
}

function resolve(flow: Flow, mode: string | undefined, args: string[]): Activation {
	if (mode === 'command' && args.length === 1) return flow.runCommand(args[0]);
	if (mode === 'fire' && args.length >= 2) {
		// Only flags guarding this exact event are accepted, so a wrong flag cannot pick a branch silently.
		const known = flow.outgoing(args[0]).flatMap((t) => (t.on === args[1] && t.when ? [guardFlag(t.when)] : []));
		const flags = args.slice(2);
		const unknown = flags.filter((f) => !known.includes(f));
		if (unknown.length)
			fail(`flag(s) ${unknown.join(', ')} not used by ${args[0]} on ${args[1]}; accepted: ${known.join(', ') || 'none'} (omit a flag to leave it unset)`);
		return flow.fire(args[0], args[1], Object.fromEntries(flags.map((f) => [f, true])));
	}
	fail('bad arguments');
}

const [mode, ...args] = process.argv.slice(2);
let flow: Flow;
let activation: Activation;
try {
	flow = load();
	activation = resolve(flow, mode, args);
} catch (e) {
	fail((e as Error).message);
}
const next = flow.outgoing(activation.block.id).map((t) => t.on + (t.when ? ` [${t.when}]` : ''));
console.log(`<!-- block: ${activation.block.id} | agent: ${activation.agent.id} | via: ${activation.cause} -->`);
if (activation.handoff)
	console.log(`<!-- HANDOFF: deliver this block to agent ${activation.agent.id}; do not execute it yourself. -->`);
console.log(activation.markdown);
console.log(`<!-- next events: ${[...new Set(next)].join(', ') || 'none'} -->`);
