import { execFileSync, spawnSync } from 'node:child_process';
import { existsSync, readFileSync } from 'node:fs';
import { join, posix } from 'node:path';

export type Journey = { name: string; why: string; tests: { spec: string; match: string; count: number; webkit?: boolean }[] };
export type Rules = { ignore: string[]; noImpact: string[]; shared: string[]; cspFull: string[]; gateway: string[] };
export type Category = { specs: string[]; sources: string[] };
export type Manifest = {
	budget: { maxJourneyTests: number; maxChromiumSeconds: number; measured: { tests: number; chromiumSeconds: number } };
	allowedCoupling?: { from: string; to: string; why: string }[]; webkitSpecs: string[]; projectFiles?: Record<string, string>; alsoRuns?: { category: string; files: string[]; why: string }[]; cspIgnoredSpecs: string[]; journeys: Journey[]; categories: Record<string, Category>; rules: Rules;
};
export type Effect = { kind: 'ignore' | 'outside' | 'all' | 'category' | 'cspFull' | 'gateway' | 'unknown'; rule: string; category?: string; also?: boolean };
export type Plan = { all: boolean; categories: string[]; cspFull: boolean; gateway: boolean; effects: [string, Effect[]][] };
export type Step = { name: string; env?: Record<string, string>; cmd: string[] };

const APP = 'apps/org-ui';
const compiled = new Map<string, RegExp>();

/** Glob subset: `**` (any depth), `*` (within a segment), `{a,b}`; brackets and parentheses are literal (SvelteKit route names). */
export function globToRegExp(glob: string): RegExp {
	let out = '';
	let depth = 0;
	for (let i = 0; i < glob.length; i++) {
		const c = glob[i];
		if (c === '*' && glob[i + 1] === '*') { i++; if (glob[i + 1] === '/') { i++; out += '(?:.*/)?'; } else out += '.*'; }
		else if (c === '*') out += '[^/]*';
		else if (c === '{') { depth++; out += '(?:'; }
		else if (c === '}' && depth) { depth--; out += ')'; }
		else if (c === ',' && depth) out += '|';
		else out += c.replace(/[.+?^$()|[\]\\]/g, '\\$&');
	}
	return new RegExp(`^${out}$`);
}

export function matches(glob: string, path: string): boolean {
	let re = compiled.get(glob);
	if (!re) compiled.set(glob, re = globToRegExp(glob));
	return re.test(path);
}

/** `file` is repository-relative (as git prints it); rules are relative to apps/org-ui. */
export function classify(file: string, m: Manifest): Effect[] {
	// Not a clean repository-relative path (absolute, backslash, `..`, NUL, git-quoted): never guess, run everything.
	if (/^[/"]|\\|\0|(^|\/)\.\.(\/|$)/.test(file)) return [{ kind: 'unknown', rule: 'unrecognizable path: full suite (fail closed)' }];
	const path = posix.relative(APP, file);
	const first = (globs: string[]) => globs.find((glob) => matches(glob, path));
	const ignored = first(m.rules.ignore);
	if (ignored) return [{ kind: 'ignore', rule: `ignore ${ignored}` }];
	const out: Effect[] = [];
	const shared = first(m.rules.shared);
	if (shared) out.push({ kind: 'all', rule: `shared ${shared}: full suite` });
	for (const [name, category] of Object.entries(m.categories)) {
		const hit = category.sources.find((glob) => matches(glob, path)) ?? category.specs.map((glob) => `e2e/${glob}`).find((glob) => matches(glob, path));
		if (hit) out.push({ kind: 'category', category: name, rule: `category ${name} (${hit})` });
	}
	// Files that another area's tests exercise without owning them: the category runs in addition.
	for (const extra of m.alsoRuns ?? []) {
		const glob = extra.files.find((candidate) => matches(candidate, path));
		if (glob) out.push({ kind: 'category', category: extra.category, also: true, rule: `category ${extra.category} also runs (${glob}): ${extra.why}` });
	}
	const csp = first(m.rules.cspFull);
	if (csp) out.push({ kind: 'cspFull', rule: `CSP/build ${csp}: full interactive CSP` });
	const gateway = first(m.rules.gateway);
	if (gateway) out.push({ kind: 'gateway', rule: `gateway/CSP/build ${gateway}: gateway CSP` });
	if (out.length) return out;
	const outside = first(m.rules.noImpact);
	return [outside ? { kind: 'outside', rule: `listed as no UI impact (${outside})` } : { kind: 'unknown', rule: 'unknown UI path: full suite (fail closed)' }];
}

export function plan(files: string[], m: Manifest): Plan {
	const effects = files.map((file) => [file, classify(file, m)] as [string, Effect[]]);
	const all = effects.flatMap(([, list]) => list);
	const has = (...kinds: Effect['kind'][]) => all.some((effect) => kinds.includes(effect.kind));
	const full = has('all', 'unknown');
	const categories = full ? Object.keys(m.categories) : [...new Set(all.flatMap((effect) => (effect.category ? [effect.category] : [])))];
	return { all: full, categories, cspFull: full || has('cspFull'), gateway: full || has('gateway'), effects };
}

/** The test files in e2e/: every `*.spec.ts`, plus the files only a dedicated Playwright project runs (`projectFiles`). */
export function specFiles(names: string[], m: Manifest): string[] {
	return names.filter((name) => name.endsWith('.spec.ts') || name in (m.projectFiles ?? {})).sort();
}

/** The spec files of the given categories, each once, even when two categories list the same file. */
export function categorySpecs(m: Manifest, categories: string[], specs: string[]): string[] {
	return specs.filter((spec) => categories.some((name) => m.categories[name].specs.some((glob) => matches(glob, spec))));
}

/** Reads the manifest; a missing or unparseable file is reported, not thrown, so the caller can fail closed. */
export function loadManifest(file: string): { manifest: Manifest } | { error: string } {
	try {
		return { manifest: JSON.parse(readFileSync(file, 'utf8')) as Manifest };
	} catch (error) {
		return { error: `cannot read the gate manifest (${error instanceof Error ? error.message.split('\n')[0] : error})` };
	}
}

/** Runs the steps in order and stops at the first one that is not a clean exit; returns the exit code (a step killed by a signal, status null, is 1). */
export function runSteps(list: Step[], cwd: string, spawn: typeof spawnSync = spawnSync, log: (line: string) => void = console.log, describe: (step: Step) => string = (step) => step.cmd.join(' ')): number {
	for (const step of list) {
		log(`\n== ${step.name}\n${describe(step)}`);
		const result = spawn(step.cmd[0], step.cmd.slice(1), { cwd, stdio: 'inherit', env: { ...process.env, ...step.env } });
		if (result.status !== 0) return result.status ?? 1;
	}
	return 0;
}

/** The Playwright invocations for a plan, in run order. `specs` are the file names in e2e/. */
export function steps(p: Plan, m: Manifest, specs: string[], { webkit = true, extra = [] as string[] } = {}): Step[] {
	const env = webkit ? { PW_WEBKIT: '1' } : undefined;
	const pw = (...args: string[]) => ['bun', 'x', 'playwright', 'test', ...args, ...extra];
	const csp = (...args: string[]) => pw('--config', 'playwright.csp.config.ts', ...args);
	const anchored = (list: string[]) => list.map((spec) => `e2e/${spec.replace(/\./g, '\\.')}$`);
	// A category that lists no spec file would silently run its journeys only.
	for (const name of p.categories) if (!categorySpecs(m, [name], specs).length) throw new Error(`category ${name} lists no spec file in e2e/`);
	const chosen = categorySpecs(m, p.categories, specs);
	const files = anchored(chosen);
	// csp-chromium-full ignores some specs; a step with nothing left would end in "No tests found".
	const cspFiles = anchored(chosen.filter((spec) => !m.cspIgnoredSpecs.includes(spec) && !(spec in (m.projectFiles ?? {}))));
	// Files only a dedicated project runs need that project beside the ordinary ones.
	const fileProjects = [...new Set(chosen.flatMap((spec) => (m.projectFiles?.[spec] ? [`--project=${m.projectFiles[spec]}`] : [])))];
	const projects = ['--project=chromium', '--project=production', ...(webkit ? ['--project=webkit'] : [])];
	const out: Step[] = [];
	if (p.all) out.push({ name: 'ordinary: full suite', env, cmd: pw() });
	else {
		out.push({ name: 'ordinary: journeys', env, cmd: pw('--grep', '@journey', ...projects) });
		if (files.length) out.push({ name: `ordinary: ${p.categories.join(', ')}`, env, cmd: pw('--grep-invert', '@journey', ...projects, ...fileProjects, ...files) });
	}
	if (p.cspFull) out.push({ name: 'csp: full interactive', cmd: csp() });
	else if (p.categories.length) {
		out.push({ name: 'csp: CSP-specific specs', cmd: csp('--project=csp-chromium', '--project=csp-webkit', '--project=csp-firefox') });
		out.push({ name: 'csp: journeys', cmd: csp('--project=csp-chromium-full', '--grep', '@journey') });
		if (cspFiles.length) out.push({ name: `csp: ${p.categories.join(', ')}`, cmd: csp('--project=csp-chromium-full', '--grep-invert', '@journey', ...cspFiles) });
	}
	if (p.gateway) out.push({ name: 'gateway CSP (3 tests)', cmd: ['bun', 'scripts/csp-gateway.ts', ...extra] });
	return out;
}

const git = (cwd: string, ...args: string[]) => execFileSync('git', args, { cwd, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] });
const nul = (output: string) => output.split('\0').filter(Boolean);
/** Stands in for the changes when they cannot be determined; it is unrecognizable on purpose, so the plan is the full suite. */
export const UNKNOWN_CHANGES = '/git-unavailable';

/** Committed (base...HEAD), staged/unstaged and untracked files, repository-relative, NUL-separated so odd names stay intact. No renames: both sides of a move count. */
export function changedFiles(base: string, cwd: string): string[] {
	const root = git(cwd, 'rev-parse', '--show-toplevel').trim();
	const sha = git(root, 'rev-parse', '--verify', '--end-of-options', `${base}^{commit}`).trim();
	return [...new Set([
		...nul(git(root, 'diff', '--name-only', '-z', '--no-renames', `${sha}...HEAD`, '--')),
		...nul(git(root, 'diff', '--name-only', '-z', '--no-renames', 'HEAD', '--')),
		...nul(git(root, 'ls-files', '--others', '--exclude-standard', '-z', '--full-name', '--', ':/'))
	])].sort();
}

/** `changedFiles`, but any git trouble (missing base, shallow clone, no repository) yields the full-suite marker. */
export function changedOrUnknown(base: string, cwd: string): { files: string[]; warning?: string } {
	try { return { files: changedFiles(base, cwd) }; } catch (error) {
		return { files: [UNKNOWN_CHANGES], warning: `cannot list changed files (${error instanceof Error ? error.message.split('\n')[0] : error}); running the full suite` };
	}
}

/** Paths typed on the command line must be repository-relative files that exist now or in HEAD (a deletion); anything else is unverified, so unknown. */
export function explicitFiles(files: string[], cwd: string): { files: string[]; warnings: string[] } {
	let root = '';
	try { root = git(cwd, 'rev-parse', '--show-toplevel').trim(); } catch { /* every path is then unverified */ }
	const known = (file: string) => {
		if (!root || !file || /^[/"]|\\|\0|(^|\/)\.\.(\/|$)/.test(file)) return false;
		if (existsSync(join(root, file))) return true;
		try { git(root, 'cat-file', '-e', `HEAD:${file}`); return true; } catch { return false; }
	};
	const bad = files.filter((file) => !known(file));
	return { files: files.map((file) => (bad.includes(file) ? `/unverified: ${file}` : file)), warnings: bad.map((file) => `"${file}" is not a repository-relative path of an existing file (e.g. apps/org-ui/src/...); treating it as unknown, so the full suite`) };
}
