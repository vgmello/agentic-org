import { execFileSync, spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, posix, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';
import { categorySpecs, changedFiles, changedOrUnknown, classify, explicitFiles, loadManifest, matches, plan, runSteps, specFiles, steps, UNKNOWN_CHANGES, type Manifest, type Step } from '../../../../scripts/ui-gate-lib';
import { parseArgs } from '../../../../scripts/ui-gate';

const app = fileURLToPath(new URL('../../../../', import.meta.url));
const read = (path: string) => readFileSync(join(app, path), 'utf8');
const manifest: Manifest & { journeys: Manifest['journeys'] } = JSON.parse(read('e2e/gate-manifest.json'));
const specs = specFiles(readdirSync(join(app, 'e2e')), manifest);
const numberedDeclarations = (spec: string) => read(`e2e/${spec}`).split('\n').map((text, index) => ({ text, line: index + 1 })).filter(({ text }) => /\btest\(/.test(text));
const declarations = (spec: string) => numberedDeclarations(spec).map(({ text }) => text);
const ui = (path: string) => `apps/org-ui/${path}`;
const names = (files: string[], options = {}) => steps(plan(files, manifest), manifest, specs, options).map((step) => step.name);

// WebKit coverage of keyboard and focus. Playwright itself decides the universe and the selection: one
// `--list --reporter=json` over every project (PW_WEBKIT=1) gives each test's loaded file, declaration (file:line), tags
// and project. The universe is every file some project loads; a file is on WebKit when the webkit project lists one of
// its tests. A test is keyed by the file that loads it as well as where it is declared, so a helper that declares tests
// for several runner files counts once per runner. A file exercises
// keyboard or focus when its source, or that of a local e2e module it imports (static, re-export or dynamic relative
// imports, followed transitively), matches KEYBOARD_OR_FOCUS. Each such file needs a test the webkit project selects or a
// webkitExempt entry with a reason; a @webkit tag the webkit project never selects counts for nothing (and fails a1).
const KEYBOARD_OR_FOCUS = /keyboard\.|\.press(?:Sequentially)?\(|\.type\(|toBeFocused|\.(?:focus|blur)\(|activeElement|hasFocus\(|:focus|dispatchEvent\(\s*['"`](?:key|focus|blur)|KeyboardEvent|FocusEvent|\bfocus(?:in|out)\b/;
type ListedTest = { file: string; declared: string; title: string; project: string; tags: string[]; skipped?: boolean };
type PlaywrightSuite = { title?: string; file?: string; specs?: { title: string; file: string; line: number; tags?: string[]; tests: { projectName: string; expectedStatus?: string }[] }[]; suites?: PlaywrightSuite[] };
type ListReport = { config?: { projects?: { name: string; testIgnore?: unknown[] }[] }; suites?: PlaywrightSuite[]; errors?: { message?: string }[] };
/**
 * Flattens Playwright's JSON list into one row per test per project; `file` is the loaded file (the top-level suite) and
 * `title` the describe titles and the test title below it, so tests a loop declares on one line stay apart.
 */
const listedTests = (report: ListReport): ListedTest[] => {
	const rows: ListedTest[] = [];
	const walk = (file: string, suite: PlaywrightSuite, path: string[]) => {
		for (const spec of suite.specs ?? [])
			for (const test of spec.tests) rows.push({ file, declared: `${spec.file}:${spec.line}`, title: [...path, spec.title].join(' › '), project: test.projectName, tags: spec.tags ?? [], ...(test.expectedStatus === 'skipped' ? { skipped: true } : {}) });
		(suite.suites ?? []).forEach((child) => walk(file, child, [...path, child.title ?? '']));
	};
	(report.suites ?? []).forEach((suite) => walk(suite.file ?? '', suite, []));
	return rows;
};
/**
 * Pure: the files Playwright loads, the files the webkit project runs, what it selects vs what carries @webkit in any
 * project (keyed by declaration, prefixed with the loaded file when a helper declares it), and the files the production
 * project runs that WebKit also runs (production-unavailable runs only against the static release).
 */
const webkitSelection = (tests: ListedTest[]) => {
	const at = ({ file, declared, title }: ListedTest) => `${declared.startsWith(`${file}:`) ? declared : `${file} › ${declared}`} › ${title}`;
	const webkit = tests.filter(({ project }) => project === 'webkit');
	// A file is on WebKit only through a test WebKit will run: a statically skipped (test.skip/fixme) test does not count.
	const onWebkit = new Set(webkit.filter(({ skipped }) => !skipped).map(({ file }) => file));
	const productionFiles = [...new Set(tests.filter(({ project }) => project === 'production').map(({ file }) => file))].sort();
	return {
		files: [...new Set(tests.map(({ file }) => file))].sort(),
		declaredFiles: [...new Set(tests.map(({ declared }) => declared.replace(/:\d+$/, '')))].sort(),
		onWebkit,
		selected: [...new Set(webkit.map(at))].sort(),
		tagged: [...new Set(tests.filter(({ tags }) => tags.includes('webkit')).map(at))].sort(),
		productionFiles,
		productionOnWebkit: productionFiles.filter((file) => onWebkit.has(file))
	};
};
// A one-line string literal, scanned in order so a closing quote never opens the next match.
const STRING_LITERAL = /(['"`])((?:\\.|(?!\1)[^\\\n])*)\1/g;
const WEBKIT_TAG = /(?:^|\s)@webkit(?![\w-])/;
// A test's title (its first argument).
const TEST_TITLE = /\btest(?:\.(?:only|skip|fixme|fail|slow))?\(\s*(['"`])((?:\\.|(?!\1)[^\\\n])*)\1/g;
// A test or describe call of the file's own (not `x.test(`, such as a regex test).
const TEST_CALL = /(?<![\w$.])test(?:\.(?:only|skip|fixme|fail|slow)|\.describe(?:\.(?:only|skip|fixme|serial|parallel))*)?\s*\(/;
// A `tag:` literal: a string or a one-line array of strings. The array form stops at a `[` too, so a line of unclosed `tag:[` is scanned once, not once per `tag:`.
const TAG_OPTION = /\btag\s*:\s*(\[[^\][\n]*\]|(['"`])(?:\\.|(?!\2)[^\\\n])*\2)/g;
/**
 * Whether a source declares a test carrying @webkit: in a test's title, or in a `tag:` literal in a file that calls test
 * or describe (so a test using a tag object defined beside it counts, and a helper that only holds the object does not).
 */
const hasWebkitTag = (source: string) =>
	[...source.matchAll(TEST_TITLE)].some(([, , title]) => WEBKIT_TAG.test(title)) ||
	(TEST_CALL.test(source) && [...source.matchAll(TAG_OPTION)].some(([, option]) => [...option.matchAll(STRING_LITERAL)].some(([, , body]) => WEBKIT_TAG.test(body))));
/** Pure: e2e sources that declare a @webkit test (tag option or title) although no project loads them and no listed test is declared in them. */
const orphanWebkitTags = ({ files, declaredFiles, sources }: { files: string[]; declaredFiles: string[]; sources: Record<string, string> }) =>
	Object.keys(sources).filter((file) => !files.includes(file) && !declaredFiles.includes(file) && hasWebkitTag(sources[file])).sort();
/** Pure: a project's resolved testIgnore patterns, as Playwright reports them (whatever the config's formatting). */
const projectIgnores = (report: ListReport, project: string) => (report.config?.projects ?? []).find(({ name }) => name === project)?.testIgnore?.map(String) ?? [];
const REPORTER_ENV = ['PLAYWRIGHT_JSON_OUTPUT_DIR', 'PLAYWRIGHT_JSON_OUTPUT_NAME', 'PW_TEST_REPORTER'];
/**
 * The child's environment: PW_WEBKIT=1 adds the webkit project, the JSON report goes to `out` (so nothing a spec or the
 * config prints on stdout can corrupt it), and the caller's reporter settings (output dir/name, an extra reporter) are dropped.
 */
const listEnv = (env: Record<string, string | undefined>, out: string) => ({ ...Object.fromEntries(Object.entries(env).filter(([key]) => !REPORTER_ENV.includes(key))), PLAYWRIGHT_JSON_OUTPUT_FILE: out, PW_WEBKIT: '1' });
/** Pure: the report a `--list --reporter=json` run wrote (`report`, null if none), or an error carrying Playwright's own errors (or its output). */
const readList = ({ status, report: text, stdout, stderr, error }: { status: number | null; report: string | null; stdout: string | null; stderr: string | null; error?: Error }): ListReport => {
	// Errors lead (a config SyntaxError is stderr's first line); a long stderr keeps its head and its last lines.
	const err = stderr ?? '';
	const output = `stdout starts ${JSON.stringify((stdout ?? '').slice(0, 500))}; stderr ${err.length > 2500 ? `${err.slice(0, 2000)} … ${err.slice(-500)}` : err}`;
	let report: unknown;
	try {
		report = JSON.parse(text ?? '');
	} catch {
		report = undefined;
	}
	if (!report || typeof report !== 'object') throw new Error(`playwright --list wrote no JSON report (exit ${status}${error ? `, ${error.message}` : ''}): ${output}`);
	const errors = ((report as ListReport).errors ?? []).map((e) => e.message ?? JSON.stringify(e));
	if (errors.length > 0 || status !== 0) throw new Error(`playwright --list failed (exit ${status}): ${errors.join('\n') || output}`);
	return report as ListReport;
};
type WebkitCoverage = { files: string[]; sources: Record<string, string>; onWebkit: Set<string>; exempt: { spec: string; why: string }[] };
const usesKeyboardOrFocus = (file: string, sources: Record<string, string>, seen = new Set<string>()): boolean => {
	if (seen.has(file) || !(file in sources)) return false;
	seen.add(file);
	if (KEYBOARD_OR_FOCUS.test(sources[file])) return true;
	return [...sources[file].matchAll(/\b(?:from|import|require)\s*(?:\(\s*)?['"`](\.{1,2}\/[^'"`]+)['"`]/g)].some(([, spec]) => {
		const base = posix.normalize(posix.join(posix.dirname(file), spec));
		return [base, `${base}.ts`, `${base}.js`, `${base}.mjs`, `${base}/index.ts`, `${base}/index.js`, base.replace(/\.([mc]?)js$/, '.$1ts')].some((candidate) => usesKeyboardOrFocus(candidate, sources, seen));
	});
};
/** Pure check: files that exercise keyboard or focus without running on WebKit or an exemption, and exemptions that are not real. */
const webkitCoverage = ({ files, sources, onWebkit, exempt }: WebkitCoverage) => {
	const focus = files.filter((file) => usesKeyboardOrFocus(file, sources));
	const listed = new Set(exempt.map(({ spec }) => spec));
	const missing = focus.filter((file) => !onWebkit.has(file) && !listed.has(file));
	const stale: string[] = [];
	exempt.forEach(({ spec, why }, index) => {
		if (!files.includes(spec)) stale.push(`${spec}: not a file Playwright runs`);
		else if (!focus.includes(spec)) stale.push(`${spec}: does not exercise keyboard or focus, so drop its exemption`);
		if (onWebkit.has(spec)) stale.push(`${spec}: runs on WebKit, so it cannot also be exempt`);
		if ((why ?? '').trim().length < 20) stale.push(`${spec}: give a reason of at least 20 characters`);
		if (exempt.findIndex((other) => other.spec === spec) !== index) stale.push(`${spec}: listed twice`);
	});
	return { missing, stale };
};
let playwrightReport: ListReport | undefined;
/** Playwright's own list over every project, spawned once per test file run (no browsers or web server; about 2 s). */
const playwrightList = () => {
	if (!playwrightReport) {
		const dir = mkdtempSync(join(tmpdir(), 'ui-gate-list-'));
		try {
			const out = join(dir, 'list.json');
			const run = spawnSync('bunx', ['playwright', 'test', '--list', '--reporter=json'], { cwd: app, encoding: 'utf8', env: listEnv(process.env, out), timeout: 150_000, maxBuffer: 64 * 1024 * 1024 });
			playwrightReport = readList({ ...run, report: existsSync(out) ? readFileSync(out, 'utf8') : null });
		} finally {
			rmSync(dir, { recursive: true, force: true });
		}
	}
	return { ...webkitSelection(listedTests(playwrightReport)), webkitIgnores: projectIgnores(playwrightReport, 'webkit') };
};
const e2eSources = (dir = ''): Record<string, string> =>
	Object.assign({}, ...readdirSync(join(app, 'e2e', dir), { withFileTypes: true }).filter((entry) => entry.name !== 'node_modules').map((entry) => {
		const file = dir ? `${dir}/${entry.name}` : entry.name;
		if (entry.isDirectory()) return e2eSources(file);
		return entry.isFile() && /\.[cm]?[jt]sx?$/.test(entry.name) ? { [file]: read(`e2e/${file}`) } : {};
	}));
const repoWebkitCoverage = (): WebkitCoverage => {
	const { files, onWebkit } = playwrightList();
	return { files, sources: e2eSources(), onWebkit, exempt: manifest.webkitExempt ?? [] };
};

describe('gate manifest guard', () => {
	it('assigns every e2e spec file to exactly one category', () => {
		for (const spec of specs) {
			const owners = Object.entries(manifest.categories).filter(([, category]) => category.specs.some((glob) => matches(glob, spec))).map(([name]) => name);
			expect(owners, `${spec} must be in exactly one category of e2e/gate-manifest.json`).toHaveLength(1);
		}
	});

	it('has no category glob that matches nothing', () => {
		for (const [name, category] of Object.entries(manifest.categories)) {
			for (const glob of category.specs) expect(specs.some((spec) => matches(glob, spec)), `${name}: ${glob}`).toBe(true);
		}
	});

	it('gives every category at least one spec file, so it never runs its journeys only', () => {
		for (const [name, category] of Object.entries(manifest.categories)) {
			expect(category.specs.length, name).toBeGreaterThan(0);
			expect(categorySpecs(manifest, [name], specs).length, `${name} lists no spec file in e2e/`).toBeGreaterThan(0);
		}
	});

	it('points each journey at exactly one tagged test declaration', () => {
		for (const journey of manifest.journeys) {
			for (const { spec, match, count } of journey.tests) {
				const found = declarations(spec).filter((line) => line.includes(match));
				expect(found, `${journey.name}: ${spec} "${match}" must match one test( line`).toHaveLength(1);
				expect(found[0], `${journey.name}: "${match}" must carry @journey`).toContain("'@journey'");
				expect(count).toBeGreaterThan(0);
			}
		}
	});

	it('has no @journey test missing from the manifest', () => {
		const tagged = specs.flatMap((spec) => declarations(spec).filter((line) => line.includes('@journey')));
		expect(tagged).toHaveLength(manifest.journeys.flatMap((journey) => journey.tests).length);
		expect(new Set(manifest.journeys.map((journey) => journey.name)).size).toBe(manifest.journeys.length);
	});

	it('keeps the journeys inside the count and time budget', () => {
		const total = manifest.journeys.flatMap((journey) => journey.tests).reduce((sum, test) => sum + test.count, 0);
		const { budget } = manifest;
		expect(total).toBeLessThanOrEqual(budget.maxJourneyTests);
		expect(budget.measured.tests, 'run the journeys, then record tests and seconds in budget.measured').toBe(total);
		expect(budget.measured.chromiumSeconds).toBeGreaterThan(0);
		expect(budget.measured.chromiumSeconds).toBeLessThanOrEqual(budget.maxChromiumSeconds);
	});

	it('makes the WebKit project list exactly the tests that carry @webkit, in every file any project runs', { timeout: 180_000 }, () => {
		// Playwright's own selection and tags, so a tag that does not reach the project (a file the webkit project never
		// selects, such as a *-default-font.ts project file) or a grep that reaches more is caught.
		const { selected, tagged, productionFiles, productionOnWebkit, webkitIgnores, files, declaredFiles } = playwrightList();
		expect(orphanWebkitTags({ files, declaredFiles, sources: e2eSources() }), 'a @webkit test declared in an e2e file whose tests no Playwright project lists (name it *.spec.ts, or call it from one)').toEqual([]);
		expect(selected, 'tag each WebKit case on its test( line with @webkit, in a file the webkit project runs; nothing else may be selected').toEqual(tagged);
		expect(tagged.length).toBeGreaterThan(0);
		expect(productionFiles.length, 'the production project lists no test').toBeGreaterThan(0);
		expect(productionOnWebkit, 'the webkit project must ignore what only runs against the static release (its testIgnore)').toEqual([]);
		expect(webkitIgnores, 'production-unavailable runs only against the static release').toContain('production-unavailable.spec.ts');
	});

	it('selects WebKit cases by the @webkit tag only, with no testMatch or BUG-id grep', () => {
		const project = read('playwright.config.ts').match(/\.\.\.\(webkit \? \[\{[\s\S]*?\}\] : \[\]\)/)![0];
		expect(project).not.toContain('testMatch');
		expect(project).not.toMatch(/BUG-\d/);
	});

	it('flags exactly the journeys whose test( line carries @webkit', () => {
		for (const { spec, match, webkit } of manifest.journeys.flatMap((journey) => journey.tests)) {
			const line = declarations(spec).find((candidate) => candidate.includes(match))!;
			expect(!!webkit, `${spec} "${match}": webkit flag must equal whether its test( line carries '@webkit'`).toBe(line.includes("'@webkit'"));
		}
	});

	it('runs every keyboard or focus file on WebKit, or exempts it with a reason', { timeout: 180_000 }, () => {
		const coverage = repoWebkitCoverage();
		expect(coverage.files.length, 'playwright --list found no test file').toBeGreaterThan(0);
		expect(coverage.files.filter((file) => !(file in coverage.sources)), 'files Playwright runs whose source the e2e walk did not read').toEqual([]);
		const { missing } = webkitCoverage(coverage);
		expect(missing.map((file) => `${file}: exercises keyboard or focus; tag one of its tests '@webkit' (in a file the webkit project runs) or add it to webkitExempt in e2e/gate-manifest.json with a reason`)).toEqual([]);
	});

	it('keeps every webkitExempt entry real: a keyboard or focus file Playwright runs, not on WebKit, listed once, with a reason', { timeout: 180_000 }, () => {
		expect(Array.isArray(manifest.webkitExempt), 'e2e/gate-manifest.json needs a webkitExempt list').toBe(true);
		expect(webkitCoverage(repoWebkitCoverage()).stale).toEqual([]);
	});

	it('refuses an untagged, unexempted keyboard spec and follows local imports to a .focus() call', () => {
		const why = 'not yet run on WebKit (BUG-169b)';
		const sources = {
			'new.spec.ts': "test('x', async ({ page }) => { await page.keyboard.press('Tab'); });",
			'via-helper.spec.ts': "import { land } from './helpers/land';\ntest('y', async ({ page }) => land(page));",
			'helpers/land.ts': "import { again } from '../via-helper.spec';\nexport const land = (page) => page.getByRole('button').focus();",
			'plain.spec.ts': "import { proto } from '../src/lib/proto';\ntest('z', async ({ page }) => { await page.click('a'); });"
		};
		const files = ['new.spec.ts', 'via-helper.spec.ts', 'plain.spec.ts'];
		expect(webkitCoverage({ files, sources, onWebkit: new Set(), exempt: [] })).toEqual({ missing: ['new.spec.ts', 'via-helper.spec.ts'], stale: [] });
		expect(webkitCoverage({ files, sources, onWebkit: new Set(['new.spec.ts']), exempt: [{ spec: 'via-helper.spec.ts', why }] })).toEqual({ missing: [], stale: [] });
		// A non-last file violates alone while the last one is fine; only a middle exemption is stale.
		expect(webkitCoverage({ files, sources, onWebkit: new Set(['via-helper.spec.ts']), exempt: [] })).toEqual({ missing: ['new.spec.ts'], stale: [] });
		expect(webkitCoverage({ files, sources, onWebkit: new Set(), exempt: [{ spec: 'new.spec.ts', why }, { spec: 'plain.spec.ts', why }, { spec: 'via-helper.spec.ts', why }] })).toEqual({ missing: [], stale: ['plain.spec.ts: does not exercise keyboard or focus, so drop its exemption'] });
		const exempt = [{ spec: 'plain.spec.ts', why }, { spec: 'gone.spec.ts', why }, { spec: 'new.spec.ts', why }, { spec: 'via-helper.spec.ts', why: 'later' }, { spec: 'via-helper.spec.ts', why }];
		expect(webkitCoverage({ files, sources, onWebkit: new Set(['new.spec.ts']), exempt }).stale).toEqual([
			'plain.spec.ts: does not exercise keyboard or focus, so drop its exemption',
			'gone.spec.ts: not a file Playwright runs',
			'new.spec.ts: runs on WebKit, so it cannot also be exempt',
			'via-helper.spec.ts: give a reason of at least 20 characters',
			'via-helper.spec.ts: listed twice'
		]);
	});

	it('counts a file as on WebKit only when the webkit project selects it, and pairs every @webkit tag with a selection', () => {
		// One fixture per universe class: a spec the webkit project selects (a, z), a spec it excludes (testIgnore, run by
		// another project) and a non-spec project file (focus-default-font.ts). The two middle files carry @webkit but
		// WebKit never runs them.
		const row = (file: string, line: number, project: string, tags: string[] = [], title = 't') => ({ file, declared: `${file}:${line}`, title, project, tags });
		const tests = [
			row('a.spec.ts', 3, 'chromium', ['webkit']),
			row('a.spec.ts', 3, 'webkit', ['webkit']),
			row('excluded.spec.ts', 2, 'production', ['webkit']),
			row('focus-default-font.ts', 4, 'chromium-focus-default-font', ['webkit']),
			row('z.spec.ts', 5, 'chromium'),
			row('z.spec.ts', 9, 'chromium', ['journey', 'webkit']),
			row('z.spec.ts', 9, 'webkit', ['journey', 'webkit'])
		];
		const selection = webkitSelection(tests);
		expect(selection.files).toEqual(['a.spec.ts', 'excluded.spec.ts', 'focus-default-font.ts', 'z.spec.ts']);
		expect(selection.productionFiles).toEqual(['excluded.spec.ts']);
		expect(selection.productionOnWebkit).toEqual([]);
		expect([...selection.onWebkit].sort()).toEqual(['a.spec.ts', 'z.spec.ts']);
		expect(selection.selected).toEqual(['a.spec.ts:3 › t', 'z.spec.ts:9 › t']);
		expect(selection.tagged, 'a1 then fails: the tags on excluded.spec.ts:2 and focus-default-font.ts:4 reach no WebKit run').toEqual(['a.spec.ts:3 › t', 'excluded.spec.ts:2 › t', 'focus-default-font.ts:4 › t', 'z.spec.ts:9 › t']);
		const sources = { 'a.spec.ts': "await page.keyboard.press('Tab');", 'excluded.spec.ts': "await page.keyboard.press('Tab');", 'focus-default-font.ts': 'await button.focus();', 'z.spec.ts': 'await expect(x).toBeFocused();' };
		expect(webkitCoverage({ files: selection.files, sources, onWebkit: selection.onWebkit, exempt: [] })).toEqual({ missing: ['excluded.spec.ts', 'focus-default-font.ts'], stale: [] });
		// A grep that reaches an untagged test shows up as a selection without a tag, also when a loop declares it on the
		// same line as a tagged one.
		expect(webkitSelection([...tests, row('z.spec.ts', 5, 'webkit')]).selected).toEqual(['a.spec.ts:3 › t', 'z.spec.ts:5 › t', 'z.spec.ts:9 › t']);
		const loop = webkitSelection([row('loop.spec.ts', 2, 'chromium', ['webkit'], 'loop a'), row('loop.spec.ts', 2, 'webkit', ['webkit'], 'loop a'), row('loop.spec.ts', 2, 'chromium', ['other'], 'loop b'), row('loop.spec.ts', 2, 'webkit', ['other'], 'loop b')]);
		expect(loop).toMatchObject({ selected: ['loop.spec.ts:2 › loop a', 'loop.spec.ts:2 › loop b'], tagged: ['loop.spec.ts:2 › loop a'] });
		// A file whose only WebKit test is statically skipped (test.fixme/skip) is selected but not on WebKit.
		const parked = webkitSelection([...tests, { ...row('parked.spec.ts', 7, 'webkit', ['webkit']), skipped: true }, row('parked.spec.ts', 7, 'chromium', ['webkit'])]);
		expect(parked.selected).toContain('parked.spec.ts:7 › t');
		expect(parked.onWebkit.has('parked.spec.ts')).toBe(false);
		expect(webkitCoverage({ files: parked.files, sources: { ...sources, 'parked.spec.ts': 'await page.keyboard.press("Tab");' }, onWebkit: parked.onWebkit, exempt: [] }).missing).toEqual(['excluded.spec.ts', 'focus-default-font.ts', 'parked.spec.ts']);
		// A webkit project that stops ignoring the production-only file shows up even when every selected test is tagged.
		expect(webkitSelection([...tests, row('excluded.spec.ts', 2, 'webkit', ['webkit'])]).productionOnWebkit).toEqual(['excluded.spec.ts']);
		// A helper that declares a tagged test for two runner files: only the runner WebKit loads is selected.
		const helper = (file: string, project: string) => ({ file, declared: 'hdecl.ts:2', title: 'h', project, tags: ['webkit'] });
		const shared = webkitSelection([helper('focus.spec.ts', 'chromium'), helper('focus.spec.ts', 'webkit'), helper('focus-default-font.ts', 'chromium-focus-default-font')]);
		expect(shared).toMatchObject({ files: ['focus-default-font.ts', 'focus.spec.ts'], selected: ['focus.spec.ts › hdecl.ts:2 › h'], tagged: ['focus-default-font.ts › hdecl.ts:2 › h', 'focus.spec.ts › hdecl.ts:2 › h'] });
		expect([...shared.onWebkit]).toEqual(['focus.spec.ts']);
		// The JSON list's nesting (loaded-file suite, describe suites, specs, one test per project) flattens to these rows:
		// the title path starts below the file suite, and a test declared in a helper keeps the loaded file.
		const report = {
			suites: [
				{ title: 'a.spec.ts', file: 'a.spec.ts', specs: [{ title: 't', file: 'a.spec.ts', line: 3, tags: ['webkit'], tests: [{ projectName: 'chromium', expectedStatus: 'passed' }, { projectName: 'webkit', expectedStatus: 'skipped' }] }] },
				{ title: 'z.spec.ts', file: 'z.spec.ts', suites: [{ title: 'd', suites: [{ title: 'e', specs: [{ title: 't', file: 'z.spec.ts', line: 5, tests: [{ projectName: 'chromium' }] }, { title: 'h', file: 'hdecl.ts', line: 2, tags: ['webkit'], tests: [{ projectName: 'webkit' }] }] }] }] }
			]
		};
		expect(listedTests(report)).toEqual([tests[0], { ...tests[1], skipped: true }, { ...tests[4], title: 'd › e › t' }, { file: 'z.spec.ts', declared: 'hdecl.ts:2', title: 'd › e › h', project: 'webkit', tags: ['webkit'] }]);
		// A tagged file no project loads is an orphan; a helper whose declared test a loaded file lists is not.
		const orphanSources = { 'a.spec.ts': "test('t', { tag: '@webkit' }, () => {});", 'orphan-flow.ts': "test('kb', { tag: ['@journey', '@webkit'] }, () => {});", 'hdecl.ts': "export const declare = () => test('h', { tag: '@webkit' }, () => {});", 'plain.ts': "export const x = 'webkit';", 'later.ts': 'test(`k`, { tag: `@webkit` }, () => {});', 'runner.spec.ts': "declareFor({ tag: '@webkit' });", 'title-flow.ts': "test('kb @webkit', async () => {});", 'near.ts': "test('kb', { tag: '@webkitish' }, () => {}); // mentions @webkit only in a comment",
			'consts.ts': "export const WEBKIT = '@webkit';",
			'comment-flow.ts': "// don't tag it @webkit, it's slow\ntest('x', () => {});",
			'hyphen-flow.ts': "test('x', { tag: '@webkit-only' }, () => {});",
			'array-flow.ts': "test.fixme('x', { tag: ['@journey', \"@webkit\"] }, () => {});",
			// A tag literal counts in a file that calls test or describe: a helper that only holds a shared tag object is no
			// orphan (a regex `.test(` is no test call), while a test using an object defined beside it is.
			'tags.ts': "export const WEBKIT = { tag: '@webkit' } as const;\nexport const both = { tag: ['@journey', '@webkit'] };",
			'regex-tags.ts': "export const WEBKIT = { tag: '@webkit' };\nexport const isTag = (s) => /^@/.test(s);",
			'uses-const.spec.ts': "import { WEBKIT } from './tags';\ntest('x', WEBKIT, () => {});",
			'const-flow.ts': "const ONLY = { tag: '@webkit' };\ntest('x', ONLY, () => {});",
			'describe-flow.ts': "test.describe('d', { tag: '@webkit' }, () => {});",
			'serial-flow.ts': "test.describe.serial.only('d', { tag: '@webkit' }, () => {});",
			'nested-flow.ts': "test(`k`, {\n\tannotation: { type: 'issue' },\n\ttag: '@webkit'\n}, async () => {});",
			'loop-flow.ts': "for (const name of names) test(name, { tag: '@webkit' }, () => {});" };
		// runner.spec.ts is loaded, but its only test is declared in a helper: it passes the tag, so it is no orphan.
		expect(orphanWebkitTags({ files: ['a.spec.ts', 'runner.spec.ts', 'uses-const.spec.ts'], declaredFiles: ['a.spec.ts', 'hdecl.ts'], sources: orphanSources })).toEqual(['array-flow.ts', 'const-flow.ts', 'describe-flow.ts', 'later.ts', 'loop-flow.ts', 'nested-flow.ts', 'orphan-flow.ts', 'serial-flow.ts', 'title-flow.ts']);
		// The quadratic-backtracking guard: a long whitespace run after `import` stays fast.
		const started = performance.now();
		expect(webkitCoverage({ files: ['ws.spec.ts'], sources: { 'ws.spec.ts': `import${' '.repeat(200_000)}x` }, onWebkit: new Set(), exempt: [] }).missing).toEqual([]);
		expect(performance.now() - started).toBeLessThan(2_000);
		// And a line of unclosed `tag:[` in an orphan test's options is scanned once, not once per `tag:`.
		const unclosed = performance.now();
		expect(orphanWebkitTags({ files: [], declaredFiles: [], sources: { 'open.ts': `test('x', { ${'tag:['.repeat(40_000)}` } })).toEqual([]);
		expect(performance.now() - unclosed).toBeLessThan(2_000);
		expect(webkitSelection([helper('focus.spec.ts', 'webkit')]).declaredFiles).toEqual(['hdecl.ts']);
		// The testIgnore is read from the named project only.
		const config = { projects: [{ name: 'chromium', testIgnore: ['production-unavailable.spec.ts'] }, { name: 'webkit', testIgnore: [] }, { name: 'firefox' }] };
		expect(projectIgnores({ config }, 'webkit')).toEqual([]);
		expect(projectIgnores({ config }, 'chromium')).toEqual(['production-unavailable.spec.ts']);
		expect(projectIgnores({ config }, 'firefox')).toEqual([]);
		expect(projectIgnores({}, 'webkit')).toEqual([]);
	});

	it('reads the --list report file or fails with Playwright\'s own error, whatever else reaches stdout or the environment', () => {
		const ok = { suites: [], errors: [] };
		const run = (status: number | null, report: unknown, stdout: string | null = '', stderr: string | null = '') => ({ status, report: typeof report === 'string' || report === null ? report : JSON.stringify(report), stdout, stderr });
		// Stray stdout (a console.log in a spec or the config) does not touch the report file.
		expect(readList(run(0, ok, 'loaded navigation\n'))).toEqual(ok);
		expect(() => readList(run(1, { suites: [], errors: [{ message: 'SyntaxError: Unexpected token in e2e/x.spec.ts' }] }))).toThrow('SyntaxError: Unexpected token in e2e/x.spec.ts');
		expect(() => readList(run(0, { errors: [{ message: 'no tests found' }] }))).toThrow('no tests found');
		expect(() => readList(run(1, ok, 'Listing', 'worker crashed'))).toThrow('failed (exit 1): stdout starts "Listing"; stderr worker crashed');
		const long = `SyntaxError: playwright.config.ts: Unexpected token (41:6)\n${'    at loader (node:internal/modules/esm/loader:501:31)\n'.repeat(60)}    at runTests (last frame)`;
		expect(() => readList(run(1, null, '', long))).toThrow(/stderr SyntaxError: playwright\.config\.ts: Unexpected token \(41:6\)\n[\s\S]* … [\s\S]*at runTests \(last frame\)$/);
		expect(() => readList(run(0, null, 'Listing tests:', 'json went elsewhere'))).toThrow('wrote no JSON report (exit 0): stdout starts "Listing tests:"; stderr json went elsewhere');
		expect(() => readList({ ...run(null, null, null, null), error: new Error('spawnSync bunx ENOENT') })).toThrow('wrote no JSON report (exit null, spawnSync bunx ENOENT)');
		expect(() => readList(run(0, 'null'))).toThrow('wrote no JSON report');
		expect(() => readList(run(0, '{"suites": ['))).toThrow('wrote no JSON report');
		const env = listEnv({ PATH: '/bin', PW_WEBKIT: '0', PLAYWRIGHT_JSON_OUTPUT_FILE: 'a', PLAYWRIGHT_JSON_OUTPUT_DIR: 'b', PLAYWRIGHT_JSON_OUTPUT_NAME: 'c', PW_TEST_REPORTER: 'list' }, '/tmp/ui-gate-list-x/list.json');
		expect(env).toEqual({ PATH: '/bin', PLAYWRIGHT_JSON_OUTPUT_FILE: '/tmp/ui-gate-list-x/list.json', PW_WEBKIT: '1' });
	});

	it('detects the other keyboard and focus forms, and helpers reached through .js, re-export or dynamic imports', () => {
		const forms = ["await field.pressSequentially('hi');", "await field.type('hi');", "await field.dispatchEvent('keydown');", "await field.dispatchEvent(`focus`);", 'await page.evaluate(() => document.hasFocus());', "el.dispatchEvent(new KeyboardEvent('keydown', { key: 'Tab' }))", "el.dispatchEvent(new FocusEvent('focus'))", "el.dispatchEvent(new Event('focusin'))", "el.dispatchEvent(new Event('focusout'))", 'await field.focus({ preventScroll: true });', 'await field.blur();', 'document.activeElement', "page.locator(':focus-visible')"];
		for (const form of forms) expect(webkitCoverage({ files: ['x.spec.ts'], sources: { 'x.spec.ts': form }, onWebkit: new Set(), exempt: [] }).missing, form).toEqual(['x.spec.ts']);
		for (const prose of ["test('stops focusing the composer on send', async () => {});", '// refocusing is the dialog\'s job', "const typeLabel = 'focused';"])
			expect(webkitCoverage({ files: ['x.spec.ts'], sources: { 'x.spec.ts': prose }, onWebkit: new Set(), exempt: [] }).missing, prose).toEqual([]);
		const sources = {
			'js.spec.ts': "import { go } from './helpers/go.js';",
			'dynamic.spec.ts': "const { go } = await import('./helpers/go');",
			'reexport.spec.ts': "export { go } from './helpers/go';",
			'template.spec.ts': "const { go } = await import(`./helpers/go`);",
			'require.spec.ts': "const { go } = require('./helpers/go');",
			'mjs.spec.ts': "import { go } from './helpers/mgo.mjs';",
			'jsx.spec.ts': "import { go } from './helpers/jgo';",
			'jsdir.spec.ts': "import { go } from './jsdir';",
			'helpers/jgo.js': "export const go = (page) => page.keyboard.type('x');",
			'jsdir/index.js': 'export const go = (field) => field.blur();',
			'helpers/mgo.mts': "export const go = (page) => page.keyboard.type('x');",
			'plain.spec.ts': "import { go } from './helpers/none.js';\nawait page.click('a');",
			'helpers/go.ts': "export const go = (page) => page.keyboard.type('x');",
			'helpers/none.ts': "export const go = (page) => page.click('a');"
		};
		const files = ['js.spec.ts', 'dynamic.spec.ts', 'plain.spec.ts', 'reexport.spec.ts', 'template.spec.ts', 'mjs.spec.ts', 'jsx.spec.ts', 'jsdir.spec.ts', 'require.spec.ts'];
		expect(webkitCoverage({ files, sources, onWebkit: new Set(), exempt: [] }).missing).toEqual(['js.spec.ts', 'dynamic.spec.ts', 'reexport.spec.ts', 'template.spec.ts', 'mjs.spec.ts', 'jsx.spec.ts', 'jsdir.spec.ts', 'require.spec.ts']);
	});

	it('keeps every alsoRuns entry real: a known category, a reason, and globs that match a current file', () => {
		const files = execFileSync('git', ['ls-files', '-z', '--', '.'], { cwd: app, encoding: 'utf8' }).split('\0').filter(Boolean);
		for (const extra of manifest.alsoRuns ?? []) {
			expect(Object.keys(manifest.categories), extra.category).toContain(extra.category);
			expect(extra.why.length).toBeGreaterThan(20);
			for (const glob of extra.files) expect(files.some((file) => matches(glob, file)), `alsoRuns glob matches nothing: ${glob}`).toBe(true);
		}
	});

	it('does not let an alsoRuns entry stand in for the file\'s own mapping', () => {
		// Every file an alsoRuns glob matches must also be mapped by a category, rules.shared or rules.ignore.
		const files = execFileSync('git', ['ls-files', '-z', '--', '.'], { cwd: app, encoding: 'utf8' }).split('\0').filter(Boolean);
		for (const extra of manifest.alsoRuns ?? [])
			for (const file of files.filter((candidate) => extra.files.some((glob) => matches(glob, candidate))))
				expect(classify(ui(file), manifest).some((effect) => !effect.also && ['category', 'all', 'ignore'].includes(effect.kind)), `${file} is only reached through alsoRuns`).toBe(true);
		// A file that only alsoRuns names is unmapped: the plan fails closed to the full suite.
		const lone = { ...manifest, alsoRuns: [{ category: 'projects-work', files: ['src/lib/zz-only-also.ts'], why: 'synthetic entry for the guard test' }] };
		expect(classify(ui('src/lib/zz-only-also.ts'), lone).map((effect) => effect.kind)).toEqual(['unknown']);
	});

	it('lists every test file that only a dedicated Playwright project runs', () => {
		const config = read('playwright.config.ts');
		const dedicated = Object.fromEntries([...config.matchAll(/name: '([^']+)', testMatch: '([^']+)'/g)].filter((m) => !m[2].endsWith('.spec.ts')).map((m) => [m[2], m[1]]));
		expect(manifest.projectFiles ?? {}).toEqual(dedicated);
	});

	it('keeps cspIgnoredSpecs equal to the csp-chromium-full testIgnore', () => {
		const config = read('playwright.csp.config.ts');
		const ignored = [...config.match(/csp-chromium-full[^\n]*?testIgnore:\s*\[([^\]]*)\]/)![1].matchAll(/'([^']+)'/g)].map((m) => m[1]);
		expect([...manifest.cspIgnoredSpecs].sort()).toEqual([...ignored].sort());
	});

	it('maps every current source and tooling file, so none forces a full run', () => {
		const files: string[] = [];
		const walk = (dir: string) => {
			for (const entry of readdirSync(join(app, dir))) {
				const path = join(dir, entry);
				if (['node_modules', '.svelte-kit', 'build', 'test-results', 'test-results-csp', 'test-results-csp-gateway', 'playwright-report'].includes(entry) || entry.startsWith('.')) continue;
				if (statSync(join(app, path)).isDirectory()) walk(path); else files.push(path);
			}
		};
		walk('.');
		const unknown = files.filter((file) => classify(ui(relative('.', file)), manifest).some((effect) => effect.kind === 'unknown'));
		expect(unknown, 'add these paths to a category, rules.shared or rules.ignore in e2e/gate-manifest.json').toEqual([]);
	});
	it('declares every cross-area import as shared: an owned file is only imported from areas that own it too', () => {
		const files: string[] = [];
		const walk = (dir: string) => { for (const entry of readdirSync(join(app, dir))) { const path = join(dir, entry); if (statSync(join(app, path)).isDirectory()) walk(path); else files.push(path); } };
		walk('src'); walk('e2e');
		const known = new Set(files);
		const resolveImport = (from: string, spec: string) => {
			const base = spec.startsWith('$lib/') ? `src/lib/${spec.slice(5)}` : spec.startsWith('.') ? relative(app, resolve(app, dirname(from), spec)) : '';
			return base ? ['', '.ts', '.svelte', '.svelte.ts', '.js', '/index.ts', '/index.svelte'].map((ext) => base + ext).find((candidate) => known.has(candidate)) : undefined;
		};
		const owner = (file: string): 'shared' | 'ignore' | 'unknown' | Set<string> => {
			const effects = classify(ui(file), manifest);
			if (effects.some((effect) => effect.kind === 'ignore')) return 'ignore';
			if (effects.some((effect) => effect.kind === 'all')) return 'shared';
			// alsoRuns selects a category to run; it does not make that category own (or import) the file.
			const categories = effects.flatMap((effect) => (effect.category && !effect.also ? [effect.category] : []));
			return categories.length ? new Set(categories) : 'unknown';
		};
		const allowed = manifest.allowedCoupling ?? [];
		const used = new Set<number>();
		const violations: string[] = [];
		for (const from of files.filter((file) => /\.(ts|svelte|js)$/.test(file) && !/\.test\.ts$/.test(file))) {
			const importer = owner(from);
			if (importer === 'ignore') continue;
			const text = read(from);
			expect(text, `${from}: import.meta.glob cannot be traced; import the files explicitly`).not.toContain('import.meta.glob');
			// Imports plus '/src/...' string paths (e2e specs fetch modules by URL).
			for (const match of text.matchAll(/(?:from|import)\s*\(?\s*['"]([^'"]+)['"]|['"`]\/(src\/(?:lib|routes)\/[^'"`$]+)['"`]/g)) {
				const to = match[2] ? known.has(match[2]) ? match[2] : undefined : resolveImport(from, match[1]);
				const target = to && owner(to);
				if (!to || !target || target === 'shared' || target === 'ignore' || target === 'unknown') continue;
				const edge = `${from} -> ${to}`;
				const covered = importer !== 'shared' && importer !== 'unknown' && [...importer].every((category) => target.has(category));
				if (covered) continue;
				const entry = allowed.findIndex((rule) => matches(rule.from, from) && matches(rule.to, to));
				if (entry >= 0) used.add(entry); else violations.push(`${edge} [${importer === 'shared' || importer === 'unknown' ? importer : [...importer].join('+')} imports ${[...target].join('+')}]`);
			}
		}
		// Routes each spec opens (visit/goto literals): the page's categories must include the spec's, else declare it.
		const routeFile = (segment: string) => [`src/routes/(shell)/${segment}/+page.svelte`, `src/routes/(onboarding)/${segment}/+page.svelte`].find((candidate) => known.has(candidate));
		for (const spec of files.filter((file) => /^e2e\/[^/]+\.spec\.ts$/.test(file))) {
			const visitor = owner(spec);
			for (const match of new Set([...read(spec).matchAll(/(?:visit\(\s*\w+\s*,|goto\()\s*[`'"]\/([a-z-]+)/g)].map((m) => m[1]))) {
				const page = routeFile(match);
				const target = page && owner(page);
				if (!page || !target || target === 'shared' || target === 'ignore' || target === 'unknown' || visitor === 'shared' || visitor === 'ignore' || visitor === 'unknown') continue;
				if ([...visitor].every((category) => target.has(category))) continue;
				const entry = allowed.findIndex((rule) => matches(rule.from, spec) && matches(rule.to, page));
				if (entry >= 0) used.add(entry); else violations.push(`${spec} opens /${match} -> ${page} [${[...visitor].join('+')} visits ${[...target].join('+')}]`);
			}
		}
		expect(violations, 'a file imported from another area (or from shared code) must be shared, owned by every importing category, or listed in allowedCoupling').toEqual([]);
		expect([...used].sort(), 'every allowedCoupling entry must match a real, current edge').toEqual(allowed.map((_, index) => index));
	});
});

describe('glob matching', () => {
	it('supports **, *, braces and literal route brackets', () => {
		expect(matches('**/*.md', 'README.md')).toBe(true);
		expect(matches('**/*.md', 'e2e/README-gate.md')).toBe(true);
		expect(matches('src/lib/{a,b}.svelte', 'src/lib/b.svelte')).toBe(true);
		expect(matches('src/lib/{a,b}.svelte', 'src/lib/c.svelte')).toBe(false);
		expect(matches('src/routes/(shell)/[section=section]/**', 'src/routes/(shell)/[section=section]/+page.svelte')).toBe(true);
		expect(matches('src/routes/+*', 'src/routes/(shell)/+layout.svelte')).toBe(false);
		expect(matches('src/lib/org/projects*', 'src/lib/org/projects.ts')).toBe(true);
	});

	it('matches the whole path only: the end is anchored, `*` stays inside a segment, a directory glob is not a prefix', () => {
		expect(matches('src/lib/a/**', 'src/lib/ab/x')).toBe(false);
		expect(matches('src/lib/a/**', 'src/lib/a')).toBe(false);
		expect(matches('src/lib/a/**', 'src/lib/a/x/y')).toBe(true);
		expect(matches('**/*.md', 'x.md.svelte')).toBe(false);
		expect(matches('**/*.md', 'foo.mdx')).toBe(false);
		expect(matches('src/*.ts', 'src/a/b.ts')).toBe(false);
		expect(matches('src/*.ts', 'src/a.ts')).toBe(true);
		expect(matches('src/lib/name.ts', 'src/lib/name.tsx')).toBe(false);
		expect(matches('src/lib/name.ts', 'xsrc/lib/name.ts')).toBe(false);
		expect(matches('scripts/ui-gate*.ts', 'scripts/ui-gate-x/evil.ts')).toBe(false);
	});

	it('does not let an ignore glob swallow a lookalike file: it is unknown, so the full suite', () => {
		for (const file of ['src/lib/x.test.ts.svelte', 'src/lib/x.md.svelte', 'scripts/ui-gate-x/evil.ts']) {
			expect(classify(ui(file), manifest), file).toEqual([expect.objectContaining({ kind: 'unknown' })]);
		}
	});
});

describe('selector', () => {
	it('chat change: journeys plus chat, tiered CSP, no full run', () => {
		const chosen = plan([ui('src/lib/modules/messaging/components/Rich.svelte')], manifest);
		expect(chosen).toMatchObject({ all: false, categories: ['chat'], cspFull: false, gateway: false });
		expect(names([ui('src/routes/(shell)/chat/+page.svelte')])).toEqual(['ordinary: journeys', 'ordinary: chat', 'csp: CSP-specific specs', 'csp: journeys', 'csp: chat']);
	});

	it('builds the exact Playwright commands for a chat change', () => {
		const list = steps(plan([ui('src/routes/(shell)/chat/+page.svelte')], manifest), manifest, specs);
		const pw = ['bun', 'x', 'playwright', 'test'];
		const webkitProjects = ['--project=chromium', '--project=production', '--project=webkit'];
		const chat = specs.filter((spec) => spec === 'chat.spec.ts' || spec.startsWith('chat-')).map((spec) => `e2e/${spec.replace(/\./g, '\\.')}$`);
		expect(chat).toHaveLength(13);
		const cspChat = chat.filter((file) => !file.includes('chat-followups-default-font'));
		expect(cspChat).toHaveLength(12);
		const csp = [...pw, '--config', 'playwright.csp.config.ts'];
		expect(list).toEqual([
			{ name: 'ordinary: journeys', env: { PW_WEBKIT: '1' }, cmd: [...pw, '--grep', '@journey', ...webkitProjects] },
			{ name: 'ordinary: chat', env: { PW_WEBKIT: '1' }, cmd: [...pw, '--grep-invert', '@journey', ...webkitProjects, '--project=chromium-chat-default-font', ...chat] },
			{ name: 'csp: CSP-specific specs', cmd: [...csp, '--project=csp-chromium', '--project=csp-webkit', '--project=csp-firefox'] },
			{ name: 'csp: journeys', cmd: [...csp, '--project=csp-chromium-full', '--grep', '@journey'] },
			{ name: 'csp: chat', cmd: [...csp, '--project=csp-chromium-full', '--grep-invert', '@journey', ...cspChat] }
		]);
	});

	it('builds the exact full and gateway commands', () => {
		const full = steps(plan([ui('package.json')], manifest), manifest, specs);
		expect(full).toEqual([
			{ name: 'ordinary: full suite', env: { PW_WEBKIT: '1' }, cmd: ['bun', 'x', 'playwright', 'test'] },
			{ name: 'csp: full interactive', cmd: ['bun', 'x', 'playwright', 'test', '--config', 'playwright.csp.config.ts'] },
			{ name: 'gateway CSP (3 tests)', cmd: ['bun', 'scripts/csp-gateway.ts'] }
		]);
	});

	it('does not let CSP-only patterns hide new files: only vitest tests under src/lib/csp are CSP-only, new helpers there or in the e2e CSP folders run everything', () => {
		for (const file of ['src/lib/csp/new-helper.ts', 'e2e-csp/new-helper.ts', 'e2e-csp-gateway/new-helper.ts']) expect(plan([ui(file)], manifest).all, file).toBe(true);
		expect(plan([ui('src/lib/csp/policy.test.ts')], manifest)).toMatchObject({ all: false, cspFull: false });
		expect(plan([ui('e2e-csp/collector.ts')], manifest)).toMatchObject({ all: false, cspFull: true });
	});

	it('treats unrecognizable paths as unknown, so they run everything', () => {
		// The unverified marker and git-failure marker end in a harmless-looking name; they must still be unknown, not ignored.
		expect(plan(['/unverified: a.md'], manifest).all).toBe(true);
		for (const file of ['/etc/passwd', 'a/../b.ts', '"caf\\303\\251.ts"', 'dir\\file.ts', '/git-unavailable']) expect(plan([file], manifest).all, file).toBe(true);
		expect(plan(['docs/a,b.md'], manifest).all).toBe(false);
	});

	it('runs the gateway tests for Rust inputs of the gateway and for CSP policy files, and ignores unrelated outside paths', () => {
		for (const file of ['crates/org-proto/src/lib.rs', 'crates/org-host-control/Cargo.toml', 'Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml', 'apps/org-gateway/examples/csp_harness.rs', 'config/csp/html.txt']) {
			expect(names([file]), file).toContain('gateway CSP (3 tests)');
		}
		expect(names(['apps/org-agent/src/main.rs', '.github/CODEOWNERS'])).toEqual(['ordinary: journeys']);
	});

	it('treats a manifest edit as shared, so it can never be journeys-only', () => {
		expect(plan([ui('e2e/gate-manifest.json')], manifest).all).toBe(true);
	});

	it('org-chart page, its load function and the chart fit menu item run org-chart and also the non-Chat icon spec (projects-work)', () => {
		for (const file of ['src/routes/(shell)/org-chart/+page.svelte', 'src/routes/(shell)/org-chart/+page.ts', 'src/lib/components/ChartFitMenuItem.svelte']) {
			const chosen = plan([ui(file)], manifest);
			expect(chosen.all, file).toBe(false);
			expect(chosen.categories.sort(), file).toEqual(['org-chart', 'projects-work']);
		}
		expect(plan([ui('src/lib/components/PeopleChart.svelte')], manifest).categories).toEqual(['org-chart']);
	});

	it('several areas run each category once', () => {
		const chosen = plan([ui('src/lib/offices/model.ts'), ui('src/lib/modules/processes/rules.ts'), ui('src/lib/offices/probe.ts')], manifest);
		expect(chosen.categories.sort()).toEqual(['offices', 'processes']);
	});

	it('runs the specs of every selected area, and a spec two areas list only once', () => {
		const two = steps(plan([ui('src/lib/offices/model.ts'), ui('src/lib/modules/processes/rules.ts')], manifest), manifest, specs);
		const ordinary = two.find((step) => step.name === 'ordinary: offices, processes')!;
		expect(ordinary.cmd).toEqual(expect.arrayContaining(['e2e/offices\\.spec\\.ts$', 'e2e/processes\\.spec\\.ts$', 'e2e/processes-bugs\\.spec\\.ts$', 'e2e/office-status-refresh\\.spec\\.ts$']));
		const cspStep = two.find((step) => step.name === 'csp: offices, processes')!;
		expect(cspStep.cmd).toEqual(expect.arrayContaining(['e2e/offices\\.spec\\.ts$', 'e2e/processes\\.spec\\.ts$']));
		const shared: Manifest = { ...manifest, categories: { one: { specs: ['a.spec.ts', 'both.spec.ts'], sources: [] }, two: { specs: ['both.spec.ts', 'b.spec.ts'], sources: [] } } };
		const files = ['a.spec.ts', 'b.spec.ts', 'both.spec.ts', 'other.spec.ts'];
		expect(categorySpecs(shared, ['one', 'two'], files)).toEqual(['a.spec.ts', 'b.spec.ts', 'both.spec.ts']);
		expect(categorySpecs(shared, ['two'], files)).toEqual(['b.spec.ts', 'both.spec.ts']);
		const built = steps({ all: false, categories: ['one', 'two'], cspFull: false, gateway: false, effects: [] }, shared, files).find((step) => step.name === 'ordinary: one, two')!;
		expect(built.cmd.filter((word) => word === 'e2e/both\\.spec\\.ts$')).toHaveLength(1);
		expect(built.cmd).toEqual(expect.arrayContaining(['e2e/a\\.spec\\.ts$', 'e2e/b\\.spec\\.ts$']));
	});

	it('refuses to build a step for a category that lists no spec file', () => {
		const empty: Manifest = { ...manifest, categories: { ghost: { specs: ['nothing.spec.ts'], sources: [] } } };
		expect(() => steps({ all: false, categories: ['ghost'], cspFull: false, gateway: false, effects: [] }, empty, specs)).toThrow('category ghost lists no spec file');
	});

	it.each([
		['a shared UI primitive', ui('src/lib/components/ui/button/button.svelte')],
		['the app shell', ui('src/lib/platform/shell/ShellTopbar.svelte')],
		['the shell layout', ui('src/routes/(shell)/+layout.svelte')],
		['playwright config', ui('playwright.config.ts')],
		['the lockfile', ui('bun.lock')],
		['e2e fakes', ui('e2e/fakes/org-api.ts')],
		['an unknown new top-level src path', ui('src/newthing/helper.ts')],
		['an unmapped component', ui('src/lib/components/BrandNewWidget.svelte')]
	])('%s runs the full suite', (_, file) => {
		const chosen = plan([file], manifest);
		expect(chosen).toMatchObject({ all: true, cspFull: true });
		expect(chosen.categories).toEqual(Object.keys(manifest.categories));
		expect(names([file]).slice(0, 2)).toEqual(['ordinary: full suite', 'csp: full interactive']);
	});

	it('a full plan always includes the 3 gateway tests, as the full gate did before', () => {
		for (const file of ['src/routes/+layout.ts', 'src/hooks.client.ts', 'static/favicon.svg', 'tsconfig.json', 'src/lib/components/ui/button/button.svelte', 'src/newthing/x.ts'].map(ui).concat(['.gitignore', '.cargo/config.toml'])) {
			expect(plan([file], manifest), file).toMatchObject({ all: true, cspFull: true, gateway: true });
			expect(names([file]), file).toEqual(['ordinary: full suite', 'csp: full interactive', 'gateway CSP (3 tests)']);
		}
	});

	it('docs, unit-test and non-UI changes run the journeys only', () => {
		for (const files of [['docs/design/x.md', ui('README.md')], [ui('src/lib/org/model.test.ts')], ['apps/org-agent/src/main.rs']]) {
			expect(plan(files, manifest)).toMatchObject({ all: false, categories: [], gateway: false });
			expect(names(files)).toEqual(['ordinary: journeys']);
		}
	});

	it('a spec change runs its own category', () => {
		expect(plan([ui('e2e/offices.spec.ts')], manifest).categories).toEqual(['offices']);
	});

	it('CSP/build config forces full CSP but keeps the ordinary run tiered', () => {
		expect(names([ui('e2e-csp/vite-policy.ts')])).toEqual(['ordinary: journeys', 'csp: full interactive']);
		expect(plan([ui('vite.config.ts')], manifest)).toMatchObject({ all: true, cspFull: true, gateway: true });
		expect(plan(['config/csp/html.txt'], manifest)).toMatchObject({ all: false, cspFull: true, gateway: true });
	});

	it('a gateway-only change adds the 3 gateway tests and no CSP browser run', () => {
		expect(names(['apps/org-gateway/src/lib.rs'])).toEqual(['ordinary: journeys', 'gateway CSP (3 tests)']);
	});

	it('uses the journey WebKit subset by default and can drop WebKit', () => {
		const [journeys] = steps(plan([], manifest), manifest, specs);
		expect(journeys.env).toEqual({ PW_WEBKIT: '1' });
		expect(journeys.cmd).toContain('--project=webkit');
		const [plain] = steps(plan([], manifest), manifest, specs, { webkit: false, extra: ['--workers=2'] });
		expect(plain.env).toBeUndefined();
		expect(plain.cmd).not.toContain('--project=webkit');
		expect(plain.cmd).toContain('--workers=2');
	});

	it('selects whole category spec files, anchored so bugs.spec.ts does not pull chat-bugs.spec.ts', () => {
		const step = steps(plan([ui('src/lib/org/projects.ts')], manifest), manifest, specs).find((s) => s.name.startsWith('ordinary: projects-work'))!;
		expect(step.cmd).toContain('e2e/bugs\\.spec\\.ts$');
		expect(step.cmd.filter((word) => word.startsWith('e2e/')).sort()).toEqual([
			'e2e/projects\\.spec\\.ts$', 'e2e/project-profile\\.spec\\.ts$', 'e2e/work\\.spec\\.ts$',
			'e2e/tasks\\.spec\\.ts$', 'e2e/dialog-block-title\\.spec\\.ts$', 'e2e/bugs\\.spec\\.ts$',
			'e2e/non-chat-icons\\.spec\\.ts$', 'e2e/non-chat-icons-default-font\\.ts$'
		].sort());
	});

	it('runs the dedicated-project test file with its own project, in the ordinary step only', () => {
		const list = steps(plan([ui('e2e/non-chat-icons.ts')], manifest), manifest, specs);
		const ordinary = list.find((step) => step.name === 'ordinary: projects-work')!;
		expect(ordinary.cmd).toContain('--project=chromium-default-font');
		expect(ordinary.cmd).toContain('e2e/non-chat-icons-default-font\\.ts$');
		expect(ordinary.cmd).toContain('e2e/non-chat-icons\\.spec\\.ts$');
		expect(list.find((step) => step.name === 'ordinary: journeys')!.cmd).not.toContain('--project=chromium-default-font');
		const csp = list.find((step) => step.name === 'csp: projects-work')!;
		expect(csp.cmd).toContain('e2e/non-chat-icons\\.spec\\.ts$');
		expect(csp.cmd).not.toContain('e2e/non-chat-icons-default-font\\.ts$');
		for (const step of list.filter((candidate) => !candidate.name.startsWith('ordinary'))) expect(step.cmd, step.name).not.toContain('--project=chromium-default-font');
		expect(names([ui('e2e/org-chart.spec.ts')])).not.toContain('csp: projects-work');
		expect(steps(plan([ui('e2e/org-chart.spec.ts')], manifest), manifest, specs).find((step) => step.name === 'ordinary: org-chart')!.cmd).not.toContain('--project=chromium-default-font');
	});

	it('maps the non-Chat icon spec, its helpers and fixture to projects-work, and re-runs it when TeamsChart or its helper changes', () => {
		for (const file of ['e2e/non-chat-icons.spec.ts', 'e2e/non-chat-icons-default-font.ts', 'e2e/non-chat-icons.ts', 'e2e/fixtures/non-chat-icons-v2.2.0.json']) {
			expect(classify(ui(file), manifest), file).toEqual([expect.objectContaining({ kind: 'category', category: 'projects-work' })]);
		}
		expect(plan([ui('src/lib/components/TeamsChart.svelte')], manifest).categories.sort()).toEqual(['org-chart', 'projects-work']);
		expect(plan([ui('src/lib/components/ProjectTeams.svelte')], manifest).categories).toEqual(['projects-work']);
		expect(classify(ui('e2e/non-chat-icons-new-helper.ts'), manifest)).toEqual([expect.objectContaining({ kind: 'unknown' })]);
	});

	it('skips a CSP category step whose specs csp-chromium-full ignores, but still runs them in the ordinary projects', () => {
		const list = steps(plan([ui('e2e/production-unavailable.spec.ts')], manifest), manifest, specs).map((step) => step.name);
		expect(list).toEqual(['ordinary: journeys', 'ordinary: production', 'csp: CSP-specific specs', 'csp: journeys']);
		const mixed = steps(plan([ui('e2e/shot.spec.ts'), ui('e2e/navigation.spec.ts')], manifest), manifest, specs).find((step) => step.name === 'csp: shell-look')!;
		expect(mixed.cmd.filter((word) => word.startsWith('e2e/')).some((word) => word.includes('shot'))).toBe(false);
		expect(mixed.cmd).toContain('e2e/navigation\\.spec\\.ts$');
	});

	it('does not let a test-named file in e2e hide from the selector, and treats CI workflow edits as shared', () => {
		expect(plan([ui('e2e/new.test.ts')], manifest).all).toBe(true);
		expect(plan(['.github/workflows/ci.yml'], manifest).all).toBe(true);
		expect(plan([ui('src/lib/org/model.test.ts'), ui('src/lib/modules/messaging/x.spec.ts')], manifest).all).toBe(false);
	});
	it('sends every outside path that is not on the no-impact list to the full suite', () => {
		for (const file of ['package.json', 'bun.lock', 'bunfig.toml', '.npmrc', '.bun-version', 'packages/ui/index.ts', '.github/actions/setup/action.yml', 'new-top-level-dir/file.ts']) expect(plan([file], manifest).all, file).toBe(true);
		for (const file of ['apps/org-agent/src/main.rs', 'apps/org-server/src/main.rs', 'apps/org-office-host/src/main.rs', 'docs/design/a.png', 'spikes/claude/run.sh', 'LICENSE', '.github/CODEOWNERS']) expect(plan([file], manifest), file).toMatchObject({ all: false, gateway: false, cspFull: false, categories: [] });
	});

	it('classifies the CSP and gateway tooling files by name, and does not ignore other scripts', () => {
		const kinds = (file: string) => classify(ui(file), manifest).map((effect) => effect.kind).sort();
		expect(kinds('scripts/csp-gateway.ts')).toEqual(['gateway']);
		expect(kinds('scripts/check-build-assets.ts')).toEqual(['cspFull', 'gateway']);
		expect(kinds('e2e-csp/collector.ts')).toEqual(['cspFull']);
		expect(kinds('e2e-csp-gateway/control.ts')).toEqual(['gateway']);
		expect(kinds('scripts/ui-gate.ts')).toEqual(['ignore']);
		expect(kinds('scripts/some-other.ts')).toEqual(['unknown']);
	});

});

describe('command line', () => {
	const run = (...args: string[]) => spawnSync('bun', [join(app, 'scripts/ui-gate.ts'), ...args], { cwd: app, encoding: 'utf8' });

	it('parses own flags in both forms, rejects unknown ones, and passes arguments after -- through', () => {
		const parsed = parseArgs(['select', '--explain', '--file=a', '--file', 'b', '--workers=3', '--', '--headed']);
		expect(parsed).toMatchObject({ command: 'select', passthrough: ['--headed'] });
		expect(parsed.values.get('--file')).toEqual(['a', 'b']);
		expect(parsed.values.get('--workers')).toEqual(['3']);
		expect(parsed.flags.has('--explain')).toBe(true);
		expect(() => parseArgs(['run', '--typo'])).toThrow('unknown argument --typo');
		expect(() => parseArgs(['run', '--explain=1'])).toThrow('unknown argument');
		expect(() => parseArgs(['run', '--workers'])).toThrow('needs a value');
	});

	it('plans from explicit files: --file=, repo-relative existing paths only, two workers unless told otherwise', () => {
		const real = 'apps/org-ui/src/lib/modules/messaging/engine.ts';
		const normal = run('select', `--file=${real}`);
		expect(normal.stdout).toContain('ordinary: chat');
		expect(normal.stdout).toContain('--workers=2');
		expect(run('select', '--file', real, '--workers', '3').stdout).toContain('--workers=3');
		expect(run('select', '--file', real, '--workers=0').status).toBe(2);
		const cwdRelative = run('select', '--file', 'src/lib/modules/messaging/engine.ts');
		expect(cwdRelative.stdout).toContain('ordinary: full suite');
		expect(cwdRelative.stderr).toContain('not a repository-relative path');
		expect(run('select', '--file', 'apps/org-ui/src/lib/typo.ts').stdout).toContain('ordinary: full suite');
		const typoDoc = run('select', '--file', 'docs/typo-does-not-exist.md');
		expect(typoDoc.stdout).toContain('ordinary: full suite');
		expect(typoDoc.stderr).toContain('not a repository-relative path');
		expect(run('select', '--typo').status).toBe(2);
		expect(run('select', '--file', 'apps/org-ui/package.json', '--no-webkit').stderr).toContain('not the full gate');
		expect(run('select', '--file', real, '--no-webkit').stderr).toContain('WebKit journey subset');
	});
});

describe('running the steps', () => {
	const list: Step[] = [{ name: 'one', cmd: ['a', '1'] }, { name: 'two', env: { X: '1' }, cmd: ['b', '2'] }, { name: 'three', cmd: ['c'] }];
	const fake = (...statuses: (number | null)[]) => {
		const calls: string[] = [];
		const spawn = ((cmd: string, args: string[], options: { env: Record<string, string> }) => { calls.push(`${cmd} ${args.join(' ')}${options.env.X ? ' X' : ''}`); return { status: statuses.length ? statuses.shift() : 0 }; }) as unknown as Parameters<typeof runSteps>[2];
		return { calls, spawn };
	};

	it('exits 0 only when every step exits 0', () => {
		const { calls, spawn } = fake(0, 0, 0);
		expect(runSteps(list, '.', spawn, () => {})).toBe(0);
		expect(calls).toEqual(['a 1', 'b 2 X', 'c ']);
	});

	it('stops at a failed step and returns its status, never 0', () => {
		const failed = fake(0, 1, 0);
		expect(runSteps(list, '.', failed.spawn, () => {})).toBe(1);
		expect(failed.calls).toEqual(['a 1', 'b 2 X']);
		const killed = fake(null);
		expect(runSteps(list, '.', killed.spawn, () => {})).toBe(1);
		expect(killed.calls).toHaveLength(1);
		expect(runSteps(list, '.', fake(0, 0, 3).spawn, () => {})).toBe(3);
	});
});

describe('a manifest that cannot be read', () => {
	it('is reported by loadManifest, not thrown', () => {
		const dir = mkdtempSync(join(tmpdir(), 'ui-gate-manifest-'));
		try {
			writeFileSync(join(dir, 'broken.json'), '{ "journeys": [');
			expect(loadManifest(join(dir, 'broken.json'))).toEqual({ error: expect.stringContaining('cannot read the gate manifest') });
			expect(loadManifest(join(dir, 'missing.json'))).toEqual({ error: expect.stringContaining('cannot read the gate manifest') });
			expect(loadManifest(join(app, 'e2e/gate-manifest.json'))).toHaveProperty('manifest');
		} finally { rmSync(dir, { recursive: true, force: true }); }
	});

	it('makes the command line run the full suite (select/run) or exit 2 (areas/verify), never a quiet journeys-only run', () => {
		const dir = mkdtempSync(join(tmpdir(), 'ui-gate-broken-'));
		const sh = (...args: string[]) => execFileSync('git', ['-c', 'user.name=t', '-c', 'user.email=t@t', '-c', 'commit.gpgsign=false', ...args], { cwd: dir, encoding: 'utf8' });
		try {
			mkdirSync(join(dir, 'scripts'));
			mkdirSync(join(dir, 'e2e'));
			for (const file of ['ui-gate.ts', 'ui-gate-lib.ts']) writeFileSync(join(dir, 'scripts', file), readFileSync(join(app, 'scripts', file)));
			writeFileSync(join(dir, 'e2e/gate-manifest.json'), '{ not json');
			writeFileSync(join(dir, 'README.md'), 'docs\n');
			sh('init', '-q');
			sh('add', '-A');
			sh('commit', '-q', '-m', 'x');
			const run = (...args: string[]) => spawnSync('bun', [join(dir, 'scripts/ui-gate.ts'), ...args], { cwd: dir, encoding: 'utf8' });
			const selected = run('select', '--file', 'README.md');
			expect(selected.status).toBe(0);
			expect(selected.stdout).toContain('ordinary: full suite');
			expect(selected.stdout).toContain('gateway CSP');
			expect(selected.stderr).toContain('cannot read the gate manifest');
			for (const command of ['areas', 'verify']) {
				const result = run(command);
				expect(result.status, command).toBe(2);
				expect(result.stderr).toContain('cannot read the gate manifest');
			}
		} finally { rmSync(dir, { recursive: true, force: true }); }
	});
});

describe('the command line with a manifest that cannot be read, on a clean tree', () => {
	const realBun = execFileSync('which', ['bun'], { encoding: 'utf8' }).trim();
	const sandbox = () => {
		const dir = mkdtempSync(join(tmpdir(), 'ui-gate-clean-'));
		const sh = (...args: string[]) => execFileSync('git', ['-c', 'user.name=t', '-c', 'user.email=t@t', '-c', 'commit.gpgsign=false', ...args], { cwd: dir, encoding: 'utf8' });
		mkdirSync(join(dir, 'scripts'));
		mkdirSync(join(dir, 'e2e'));
		mkdirSync(join(dir, 'bin'));
		for (const file of ['ui-gate.ts', 'ui-gate-lib.ts']) writeFileSync(join(dir, 'scripts', file), readFileSync(join(app, 'scripts', file)));
		writeFileSync(join(dir, 'e2e/gate-manifest.json'), '{ not json');
		// A stand-in for bun in the steps: `build` succeeds, any other call logs itself and exits with the status in FAKE_STATUS.
		writeFileSync(join(dir, 'bin/bun'), '#!/bin/sh\n[ "$2" = build ] && exit 0\necho "$@" >> "$FAKE_LOG"\nexit "${FAKE_STATUS:-0}"\n', { mode: 0o755 });
		sh('init', '-q');
		sh('add', '-A');
		sh('commit', '-q', '-m', 'x');
		const run = (command: string, env: Record<string, string> = {}) => spawnSync(realBun, [join(dir, 'scripts/ui-gate.ts'), command, '--base', 'HEAD'], { cwd: dir, encoding: 'utf8', env: { ...process.env, PATH: `${join(dir, 'bin')}:${process.env.PATH}`, FAKE_LOG: join(dir, 'fake.log'), ...env } });
		return { dir, run };
	};

	it('plans the full suite, not journeys only, when nothing changed and the manifest is broken', () => {
		const { dir, run } = sandbox();
		try {
			const selected = run('select');
			expect(selected.status).toBe(0);
			expect(selected.stdout).toContain('ordinary: full suite');
			expect(selected.stdout).toContain('csp: full interactive');
			expect(selected.stdout).toContain('gateway CSP');
			expect(selected.stdout).not.toContain('ordinary: journeys');
		} finally { rmSync(dir, { recursive: true, force: true }); }
	});

	it('exits with the status of the first failing step, and 0 only when all steps passed', () => {
		const { dir, run } = sandbox();
		try {
			const red = run('run', { FAKE_STATUS: '7' });
			expect(red.status).toBe(7);
			expect(red.stdout).toContain('== ordinary: full suite');
			expect(red.stdout).not.toContain('== csp: full interactive');
			const green = run('run', { FAKE_STATUS: '0' });
			expect(green.status).toBe(0);
			expect(green.stdout).toContain('== gateway CSP (3 tests)');
		} finally { rmSync(dir, { recursive: true, force: true }); }
	});
});

describe('changed files from git', () => {
	const git = (cwd: string, ...args: string[]) => execFileSync('git', ['-c', 'user.name=t', '-c', 'user.email=t@t', '-c', 'commit.gpgsign=false', ...args], { cwd, encoding: 'utf8' });
	const sandbox = () => {
		const root = mkdtempSync(join(tmpdir(), 'ui-gate-'));
		git(root, 'init', '-q', '-b', 'main');
		mkdirSync(join(root, 'apps/org-ui/src'), { recursive: true });
		// Distinct multi-line contents, so git's rename detection really fires.
		for (const file of ['apps/org-ui/src/a.ts', 'apps/org-ui/src/gone.ts', 'README.md']) writeFileSync(join(root, file), Array.from({ length: 12 }, (_, line) => `${file} line ${line}`).join('\n') + '\n');
		git(root, 'add', '-A'); git(root, 'commit', '-qm', 'base');
		return root;
	};

	it('sees committed, staged, unstaged, deleted, renamed and untracked files, even outside the app and with odd names', () => {
		const root = sandbox();
		try {
			git(root, 'checkout', '-qb', 'work');
			writeFileSync(join(root, 'apps/org-ui/src/a.ts'), 'y\n'); git(root, 'commit', '-qam', 'edit a');
			git(root, 'mv', 'README.md', 'docs.md');
			expect(git(root, 'diff', '--name-status', '-M', 'HEAD')).toContain('R1'); // plain git would report a rename, which `--no-renames` must undo
			writeFileSync(join(root, 'apps/org-ui/src/gone.ts'), 'z\n');
			rmSync(join(root, 'apps/org-ui/src/gone.ts'));
			mkdirSync(join(root, 'config/csp'), { recursive: true });
			writeFileSync(join(root, 'config/csp/new.txt'), 'x');
			writeFileSync(join(root, 'apps/org-ui/src/caf\u00e9 \\"q\".ts'), 'x');
			const files = changedFiles('main', join(root, 'apps/org-ui'));
			expect(files).toEqual(expect.arrayContaining(['apps/org-ui/src/a.ts', 'apps/org-ui/src/gone.ts', 'README.md', 'docs.md', 'config/csp/new.txt', 'apps/org-ui/src/caf\u00e9 \\"q\".ts']));
			expect(files.some((file) => file.startsWith('"'))).toBe(false);
		} finally { rmSync(root, { recursive: true, force: true }); }
	});

	it('sees a rename that was committed on the branch as both paths', () => {
		const root = sandbox();
		try {
			git(root, 'checkout', '-qb', 'work');
			git(root, 'mv', 'README.md', 'docs.md');
			git(root, 'commit', '-qm', 'rename');
			expect(git(root, 'diff', '--name-status', '-M', 'main...HEAD')).toContain('R1');
			expect(changedFiles('main', join(root, 'apps/org-ui'))).toEqual(['README.md', 'docs.md']);
		} finally { rmSync(root, { recursive: true, force: true }); }
	});

	it('falls back to the full-suite marker when git cannot answer', () => {
		const root = sandbox();
		try {
			const missing = changedOrUnknown('origin/v1', root);
			expect(missing.files).toEqual([UNKNOWN_CHANGES]);
			expect(missing.warning).toContain('running the full suite');
			expect(changedOrUnknown('--upload-pack=x', root).files).toEqual([UNKNOWN_CHANGES]);
			expect(changedOrUnknown('main', tmpdir()).files).toEqual([UNKNOWN_CHANGES]);
			expect(plan([UNKNOWN_CHANGES], manifest).all).toBe(true);
			expect(changedOrUnknown('main', root).warning).toBeUndefined();
		} finally { rmSync(root, { recursive: true, force: true }); }
	});

	it('accepts explicit paths that exist now or in HEAD and marks the rest unknown', () => {
		const root = sandbox();
		try {
			rmSync(join(root, 'apps/org-ui/src/gone.ts'));
			const { files, warnings } = explicitFiles(['apps/org-ui/src/a.ts', 'apps/org-ui/src/gone.ts', 'src/a.ts', '../x', '/abs'], join(root, 'apps/org-ui'));
			expect(files.slice(0, 2)).toEqual(['apps/org-ui/src/a.ts', 'apps/org-ui/src/gone.ts']);
			expect(files.slice(2).every((file) => file.startsWith('/unverified: '))).toBe(true);
			expect(warnings).toHaveLength(3);
		} finally { rmSync(root, { recursive: true, force: true }); }
	});
});
