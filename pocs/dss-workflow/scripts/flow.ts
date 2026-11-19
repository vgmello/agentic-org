// Agent/host-side CLI over a persisted runner. The state file stands in for the office journal,
// so every call is a fresh process: a "restart" between any two steps.
//   bun scripts/flow.ts --state s.json --caps task,project start dss --task T-1
//   bun scripts/flow.ts --state s.json begin R1.1
//   bun scripts/flow.ts --state s.json report R1.1 context_sufficient
//   bun scripts/flow.ts --state s.json task T-1 in_review
import { existsSync, readFileSync, renameSync, writeFileSync } from 'node:fs';
import { CAPABILITY_CATALOG } from '../src/lib/engine.ts';
import { TASK_STATUSES, emptyHost, factSource, moveTask, type HostState, type TaskStatus } from '../src/lib/host.ts';
import { readFlow } from '../src/lib/load-node.ts';
import { Runner, emptyState, type RunState } from '../src/lib/runtime.ts';

const USAGE = `usage: flow.ts [--state FILE] [--caps task,project] <command>
  start <trigger> [--task ID]      start a run
  begin <step>                     agent: my turn for this step started
  report <step> <outcome> [flag|!flag..] agent: report the block's outcome
  task <id> <${TASK_STATUSES.join('|')}>   host: move a task
  standards <true|false>           host: project fact standards_defined
  tick                             time passes; expired dispatches retry
  status                           show runs`;

interface Saved {
	capabilities: string[];
	runner: RunState;
	host: HostState;
}

function fail(message: string): never {
	console.error(`error: ${message}\n${USAGE}`);
	process.exit(2);
}

const argv = process.argv.slice(2);
const option = (name: string): string | undefined => {
	const i = argv.indexOf(name);
	if (i < 0) return undefined;
	const value = argv[i + 1];
	if (value === undefined || value.startsWith('--')) fail(`${name} needs a value`);
	argv.splice(i, 2);
	return value;
};

function main() {
	const file = option('--state') ?? '.flow-state.json';
	const capsArg = option('--caps');
	const task = option('--task');
	const saved: Saved = existsSync(file)
		? (JSON.parse(readFileSync(file, 'utf8')) as Saved)
		: { capabilities: capsArg ? capsArg.split(',').filter(Boolean) : [], runner: emptyState(), host: emptyHost() };
	if (capsArg !== undefined && capsArg.split(',').filter(Boolean).join() !== saved.capabilities.join())
		fail(`state file was created with --caps ${saved.capabilities.join(',') || '(none)'}; capabilities cannot change mid-run`);
	for (const c of saved.capabilities)
		if (!CAPABILITY_CATALOG.some((k) => k.name === c)) fail(`unknown capability ${c}`);

	const dispatched: string[] = [];
	const runner = new Runner(readFlow(saved.capabilities), saved.runner, factSource(saved.host, saved.capabilities), (run, step, a) => {
		dispatched.push(
			`<!-- dispatch: run ${run.id} | step ${step.id} | agent: ${a.agent.id} | outcomes: ${runner.flow.outcomes(step.block).join(', ') || 'none'} -->\n${a.markdown}`
		);
	});

	const [cmd, ...args] = argv;
	const need = (n: number) => args.length === n || fail(`${cmd} takes ${n} argument(s)`);
	switch (cmd) {
		case 'start':
			need(1);
			console.log(`started ${runner.start(args[0], task ? { task } : {}).id}`);
			break;
		case 'begin':
			need(1);
			runner.begin(args[0]);
			console.log(`step ${args[0]} running`);
			break;
		case 'report': {
			if (args.length < 2) fail('report takes <step> <outcome> [flag|!flag...]');
			const [step, outcome, ...flags] = args;
			// `!flag` clears a flag the run holds; a bare `flag` sets it.
			const parsed = Object.fromEntries(flags.map((f) => (f.startsWith('!') ? [f.slice(1), false] : [f, true])));
			const result = runner.report(`${step}:${outcome}`, step, outcome, parsed);
			console.log(result);
			if (result.startsWith('rejected')) process.exitCode = 3;
			break;
		}
		case 'task':
			need(2);
			if (!TASK_STATUSES.includes(args[1] as TaskStatus)) fail(`status must be one of ${TASK_STATUSES.join(', ')}`);
			moveTask(saved.host, runner, args[0], args[1] as TaskStatus);
			runner.reconcile();
			console.log(`${args[0]} → ${args[1]}`);
			break;
		case 'standards':
			need(1);
			saved.host.project.standards_defined = args[0] === 'true';
			runner.reconcile();
			break;
		case 'tick':
			need(0);
			runner.tick();
			break;
		case 'status':
			need(0);
			for (const run of runner.runs) {
				const s = runner.current(run);
				console.log(`${run.id} ${run.status}${run.attention ? ` (${run.attention})` : ''}: ${s.block} [${s.state}] @${s.agent} step ${s.id}`);
			}
			break;
		default:
			fail(cmd ? `unknown command ${cmd}` : 'missing command');
	}
	for (const d of dispatched) console.log(d);
	// Write-then-rename so a crash never leaves a torn state file.
	writeFileSync(`${file}.tmp`, JSON.stringify({ ...saved, runner: runner.snapshot() }, null, 1));
	renameSync(`${file}.tmp`, file);
}

try {
	main();
} catch (e) {
	fail((e as Error).message);
}
