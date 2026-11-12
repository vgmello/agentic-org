import { execFileSync, spawnSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';
import { categorySpecs, changedFiles, changedOrUnknown, classify, explicitFiles, loadManifest, matches, plan, runSteps, specFiles, steps, UNKNOWN_CHANGES, type Manifest, type Step } from '../../../../scripts/ui-gate-lib';
import { parseArgs } from '../../../../scripts/ui-gate';

const app = fileURLToPath(new URL('../../../../', import.meta.url));
const read = (path: string) => readFileSync(join(app, path), 'utf8');
const manifest: Manifest & { journeys: Manifest['journeys'] } = JSON.parse(read('e2e/gate-manifest.json'));
const specs = specFiles(readdirSync(join(app, 'e2e')), manifest);
const declarations = (spec: string) => read(`e2e/${spec}`).split('\n').filter((line) => /\btest\(/.test(line));
const ui = (path: string) => `apps/org-ui/${path}`;
const names = (files: string[], options = {}) => steps(plan(files, manifest), manifest, specs, options).map((step) => step.name);

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

	it('keeps the WebKit subset equal to playwright.config.ts and its journeys flagged', () => {
		const config = read('playwright.config.ts');
		const testMatch = [...config.match(/testMatch:\s*\[([^\]]*)\]/)![1].matchAll(/'([^']+)'/g)].map((m) => m[1]);
		const grep = new RegExp(config.match(/grep:\s*\/(.+)\/,/)![1]);
		expect([...manifest.webkitSpecs].sort()).toEqual([...testMatch].sort());
		for (const { spec, match, webkit } of manifest.journeys.flatMap((journey) => journey.tests)) {
			const line = declarations(spec).find((candidate) => candidate.includes(match))!;
			expect(!!webkit, `${spec} "${match}": webkit flag must equal whether the WebKit project runs it`).toBe(testMatch.includes(spec) && grep.test(line));
		}
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
			const categories = effects.flatMap((effect) => (effect.category ? [effect.category] : []));
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
		expect(chat).toHaveLength(11);
		const csp = [...pw, '--config', 'playwright.csp.config.ts'];
		expect(list).toEqual([
			{ name: 'ordinary: journeys', env: { PW_WEBKIT: '1' }, cmd: [...pw, '--grep', '@journey', ...webkitProjects] },
			{ name: 'ordinary: chat', env: { PW_WEBKIT: '1' }, cmd: [...pw, '--grep-invert', '@journey', ...webkitProjects, ...chat] },
			{ name: 'csp: CSP-specific specs', cmd: [...csp, '--project=csp-chromium', '--project=csp-webkit', '--project=csp-firefox'] },
			{ name: 'csp: journeys', cmd: [...csp, '--project=csp-chromium-full', '--grep', '@journey'] },
			{ name: 'csp: chat', cmd: [...csp, '--project=csp-chromium-full', '--grep-invert', '@journey', ...chat] }
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

	it('org-chart page change maps to the org-chart category only', () => {
		expect(plan([ui('src/routes/(shell)/org-chart/+page.svelte')], manifest).categories).toEqual(['org-chart']);
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
		expect(step.cmd.filter((word) => word.startsWith('e2e/'))).toHaveLength(7);
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
