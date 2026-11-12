import { execFileSync } from 'node:child_process';
import { readdirSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { categorySpecs, changedOrUnknown, explicitFiles, loadManifest, plan, runSteps, specFiles, steps, UNKNOWN_CHANGES, type Manifest, type Step } from './ui-gate-lib';

const app = fileURLToPath(new URL('../', import.meta.url));
const loaded = loadManifest(`${app}e2e/gate-manifest.json`);
// Without a readable manifest nothing can be selected: select/run fall back to the full suite, areas/verify have nothing to report.
const manifest: Manifest = 'manifest' in loaded ? loaded.manifest : { budget: { maxJourneyTests: 0, maxChromiumSeconds: 0, measured: { tests: 0, chromiumSeconds: 0 } }, webkitExempt: [], cspIgnoredSpecs: [], journeys: [], categories: {}, rules: { ignore: [], noImpact: [], shared: [], cspFull: [], gateway: [] } };
const specs = specFiles(readdirSync(`${app}e2e`), manifest);
const quote = (word: string) => (/^[\w@./=:,-]+$/.test(word) ? word : `'${word}'`);
const show = (step: Step) => `${Object.entries(step.env ?? {}).map(([k, v]) => `${k}=${v} `).join('')}${step.cmd.map(quote).join(' ')}`;
function listAreas() {
	for (const [name, category] of Object.entries(manifest.categories)) {
		console.log(`${name}: ${categorySpecs(manifest, [name], specs).join(', ')}\n  sources: ${category.sources.join(' ') || '(none: only its own specs or a full run)'}`);
	}
	const journeys = manifest.journeys.flatMap((journey) => journey.tests);
	console.log(`journeys: ${journeys.reduce((sum, test) => sum + test.count, 0)} tests in ${manifest.journeys.length} flows (${journeys.filter((test) => test.webkit).reduce((sum, test) => sum + test.count, 0)} also on WebKit)`);
	for (const journey of manifest.journeys) console.log(`  ${journey.name}: ${journey.tests.reduce((sum, test) => sum + test.count, 0)}`);
}

type Listed = { file: string; title: string; project: string };
/** The tests a built command would run, asked of Playwright itself (`--list`), one entry per project. */
function listed(step: Step): Listed[] {
	const out = execFileSync(step.cmd[0], [...step.cmd.slice(1), '--list', '--reporter=json'], { cwd: app, encoding: 'utf8', maxBuffer: 1 << 26, env: { ...process.env, ...step.env }, stdio: ['ignore', 'pipe', 'ignore'] });
	type Suite = { file?: string; specs?: { title: string; file: string; tests?: { projectName: string }[] }[]; suites?: Suite[] };
	const walk = (suite: Suite): Listed[] => [...(suite.specs ?? []).flatMap((spec) => (spec.tests ?? [{ projectName: '' }]).map((test) => ({ file: spec.file, title: spec.title, project: test.projectName }))), ...(suite.suites ?? []).flatMap(walk)];
	return walk(JSON.parse(out));
}

/** The journeys and the category selections, built by the selector itself, must add up to exactly the whole suite, and every manifest entry must list the tests it claims. */
function verify(): number {
	const everything = Object.keys(manifest.categories);
	const [journeys, rest] = steps({ all: false, categories: everything, cspFull: false, gateway: false, effects: [] }, manifest, specs);
	const [full] = steps({ all: true, categories: everything, cspFull: true, gateway: false, effects: [] }, manifest, specs);
	const entries = manifest.journeys.flatMap((journey) => journey.tests.map((test) => ({ ...test, journey: journey.name })));
	const declared = entries.reduce((sum, test) => sum + test.count, 0);
	const onWebkit = entries.filter((test) => test.webkit).reduce((sum, test) => sum + test.count, 0);
	const [all, j, r] = [listed(full), listed(journeys), listed(rest)];
	const problems: string[] = [];
	if (j.length !== declared + onWebkit) problems.push(`journey step lists ${j.length} tests, manifest says ${declared} + ${onWebkit} on WebKit`);
	if (declared > manifest.budget.maxJourneyTests) problems.push(`${declared} journeys exceed the budget of ${manifest.budget.maxJourneyTests}`);
	if (j.length + r.length !== all.length) problems.push(`journeys (${j.length}) + categories (${r.length}) != full suite (${all.length}): a spec is in no category or a journey is not in its category`);
	for (const entry of entries) {
		const found = j.filter((test) => test.project !== 'webkit' && test.file.endsWith(entry.spec) && test.title.includes(entry.match)).length;
		if (found !== entry.count) problems.push(`${entry.journey}: ${entry.spec} "${entry.match}" lists ${found} tests, manifest count is ${entry.count}`);
	}
	console.log(`full suite: ${all.length}; journeys: ${j.length} (manifest ${declared} + ${onWebkit} WebKit, budget ${manifest.budget.maxJourneyTests}); categories without journeys: ${r.length}`);
	problems.forEach((problem) => console.error(`verify: ${problem}`));
	return problems.length ? 1 : 0;
}

const COMMANDS = ['select', 'run', 'areas', 'verify'];
const FLAGS = ['--explain', '--no-webkit'];
const VALUE_FLAGS = ['--base', '--file', '--files', '--workers'];

/** Own arguments only: `--flag value` and `--flag=value`. Anything after `--` goes to Playwright. Unknown flags are an error, never silently ignored. */
export function parseArgs(argv: string[]) {
	const dash = argv.indexOf('--');
	const own = dash < 0 ? argv : argv.slice(0, dash);
	const parsed = { command: 'select', flags: new Set<string>(), values: new Map<string, string[]>(), passthrough: dash < 0 ? [] : argv.slice(dash + 1) };
	for (let at = 0; at < own.length; at++) {
		const [name, inline] = own[at].startsWith('--') ? [own[at].split('=')[0], own[at].includes('=') ? own[at].slice(own[at].indexOf('=') + 1) : undefined] : [own[at], undefined];
		if (COMMANDS.includes(name) && !own[at].startsWith('--')) parsed.command = name;
		else if (FLAGS.includes(name) && inline === undefined) parsed.flags.add(name);
		else if (VALUE_FLAGS.includes(name)) {
			const value = inline ?? own[++at];
			if (value === undefined) throw new Error(`${name} needs a value`);
			parsed.values.set(name, [...(parsed.values.get(name) ?? []), value]);
		} else throw new Error(`unknown argument ${own[at]} (pass Playwright arguments after a second --, e.g. bun run test:e2e:affected -- --workers 2 -- --headed)`);
	}
	return parsed;
}

if (import.meta.main) {
	try {
		const args = parseArgs(process.argv.slice(2));
		if ('error' in loaded) {
			if (args.command === 'areas' || args.command === 'verify') throw new Error(loaded.error);
			console.error(`ui-gate: ${loaded.error}; running the full suite`);
		}
		const value = (name: string) => args.values.get(name) ?? [];
		const workers = value('--workers')[0] ?? '2';
		if (!/^[1-9]\d*$/.test(workers)) throw new Error(`--workers must be a positive integer, got ${workers}`);
		if (args.command === 'areas') listAreas();
		else if (args.command === 'verify') process.exitCode = verify();
		else {
			// --file <path> (repeatable) or --files <newline-separated paths>, repository-relative and existing; otherwise git. Git trouble means the full suite.
			const given = [...value('--file'), ...value('--files').flatMap((list) => list.split('\n').filter(Boolean))];
			let files: string[];
			if (given.length) {
				const checked = explicitFiles(given, app);
				checked.warnings.forEach((warning) => console.error(`ui-gate: ${warning}`));
				files = checked.files;
			} else {
				const changed = changedOrUnknown(value('--base')[0] ?? 'origin/v1', app);
				if (changed.warning) console.error(`ui-gate: ${changed.warning}`);
				files = changed.files;
			}
			const chosen = plan('error' in loaded ? [UNKNOWN_CHANGES] : files, manifest);
			const webkit = !args.flags.has('--no-webkit');
			if (!webkit) console.error(`ui-gate: warning: --no-webkit skips WebKit${chosen.all ? ', so this is not the full gate' : ' (including the WebKit journey subset)'}`);
			// Two workers by default: the shared Mac is overloaded. Choose more with --workers <n>.
			const list = steps(chosen, manifest, specs, { webkit, extra: [`--workers=${workers}`, ...args.passthrough] });
			if (args.flags.has('--explain')) {
				for (const [file, effects] of chosen.effects) console.log(`${file}\n${effects.map((effect) => `  - ${effect.rule}`).join('\n')}`);
				console.log(`plan: ${chosen.all ? 'FULL' : `journeys + ${chosen.categories.join(', ') || 'no categories'}`}${chosen.cspFull ? '; full CSP' : ''}${chosen.gateway ? '; gateway CSP' : ''}`);
			}
			if (args.command === 'select') list.forEach((step) => console.log(`${step.name}\n  ${show(step)}`));
			else {
				// Ordinary runs include the production project, which serves the static build.
				if (list.some((step) => !step.cmd.includes('--config') && step.cmd[1] === 'x')) execFileSync('bun', ['run', 'build'], { cwd: app, stdio: 'inherit' });
				process.exitCode = runSteps(list, app, undefined, undefined, show);
			}
		}
	} catch (error) {
		console.error(`ui-gate: ${error instanceof Error ? error.message : error}`);
		process.exitCode = 2;
	}
}
