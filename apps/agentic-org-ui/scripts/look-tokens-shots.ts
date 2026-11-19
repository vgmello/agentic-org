// Before/after evidence for a look-only change (the look-token codemod, a token value change):
// captures every built screen and its main mocked states at 1440 and 390 px against a running
// `bun run dev:mock -- --port <port> --strictPort`, then pixel-diffs two captures.
//
//   bun scripts/look-tokens-shots.ts capture <outDir> [--port 5364] [--urls <urls.json>]
//   bun scripts/look-tokens-shots.ts diff <beforeDir> <afterDir> [<report.md>]
//
// The first capture crawls the app from fixed seeds and writes <outDir>/urls.json; pass that file
// to the second capture with --urls so both sets cover exactly the same pages. The clock is fixed
// and animations are disabled so the same build renders the same pixels. Test data only: the
// mocked dev server serves fakes, never an organization's workspace.
import { mkdir, readFile, readdir, writeFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
import { join } from 'node:path';
import { chromium, type Browser, type Page } from '@playwright/test';

const require = createRequire(import.meta.url);
// Playwright's own bundled PNG codec; no extra dependency.
const { PNG } = require('playwright-core/lib/utilsBundle') as {
	PNG: { sync: { read(buffer: Buffer): { width: number; height: number; data: Buffer } } };
};

const widths = [1440, 390] as const;
const fixedTime = new Date('2026-11-19T12:00:00Z');

/** Route files as URL patterns, so the crawl keeps at most two concrete pages per pattern. */
async function routePatterns(): Promise<RegExp[]> {
	const out: RegExp[] = [];
	async function walk(dir: string, parts: string[]) {
		for (const entry of await readdir(dir, { withFileTypes: true })) {
			if (entry.isDirectory()) {
				const seg = entry.name;
				const next = /^\(.*\)$/.test(seg)
					? parts
					: [...parts, /^\[.*\]$/.test(seg) ? '[^/]+' : seg.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')];
				await walk(join(dir, seg), next);
			} else if (entry.name === '+page.svelte' || entry.name === '+page.ts') {
				out.push(new RegExp(`^/${parts.join('/')}$`));
			}
		}
	}
	await walk('src/routes', []);
	return out;
}

/** Seeds: the area roots plus the mocked states the fakes serve (empty, failure, unavailable, viewer). */
const seeds = [
	'/home',
	'/inbox',
	'/activity',
	'/chat',
	'/chat?c=c-general',
	'/chat?c=c-release-v1',
	'/chat?c=c-dm-sam',
	'/chat?org=empty',
	'/chat?chat=fail',
	'/work',
	'/work?tab=requests',
	'/projects',
	'/projects/atlas',
	'/teams',
	'/teams/members',
	'/teams/members?mock=empty',
	'/teams/members?mock=fails',
	'/teams/members?mock=unavailable',
	'/teams/members?mock=all-former',
	'/teams/members?add=1',
	'/teams/org-chart',
	'/org-chart',
	'/org-chart?org=empty',
	'/teams/hire',
	'/teams/hires',
	'/teams/hires?mock=empty',
	'/teams/hires?mock=fails',
	'/members/kai',
	'/members/mira',
	'/members/avery',
	'/insights',
	'/search',
	'/preferences',
	'/settings',
	'/settings/access',
	'/settings/agents',
	'/settings/costs',
	'/settings/offices',
	'/offices?mock=unreachable',
	'/offices?org=empty',
	'/offices?org=fails',
	'/offices?restart=not_ready',
	'/settings/processes',
	'/processes?processes=empty',
	'/settings/roles',
	'/settings/roles?mock=empty',
	'/settings/roles?mock=fails',
	'/settings/roles?mock=viewer',
	'/settings/roles?mock=stale',
	'/settings/roles?mock=hr-off',
	'/settings/roles?mock=unavailable',
	'/settings/roles/role-swe',
	'/settings/roles/role-swe?mock=viewer',
	'/settings/roles/no-such-role',
	'/settings/updates',
	'/no-such-area'
];

async function signedInPage(browser: Browser, base: string, width: number): Promise<Page> {
	const context = await browser.newContext({
		baseURL: base,
		viewport: { width, height: 900 },
		reducedMotion: 'reduce'
	});
	const page = await context.newPage();
	await page.clock.setFixedTime(fixedTime);
	const created = await context.request.post('/__p6-auth/new');
	const { id } = await created.json();
	await context.addCookies([{ name: 'p6_auth_case', value: id, url: base }]);
	await context.request.post('/__p6-auth/control', {
		data: {
			id,
			action: 'organizations',
			value: [
				{
					id: 'org-look',
					name: 'Look tokens',
					created_at: '2026-11-18T00:00:00Z',
					recovery: 'acknowledged'
				}
			]
		}
	});
	await visit(page, '/sign-in');
	await page.getByLabel('Username').fill('admin');
	await page.getByLabel('Password', { exact: true }).fill('private-password');
	await page.getByRole('button', { name: 'Sign in', exact: true }).click();
	await page.waitForURL(/\/welcome$/);
	await visit(page, '/create');
	await page.getByLabel('Organization name').fill('Look tokens');
	await page.getByRole('button', { name: 'Create organization' }).click();
	await page.waitForURL(/\/(agents|teams\/members)$/, { waitUntil: 'commit' });
	return page;
}

async function visit(page: Page, url: string) {
	await page.goto(url);
	await page.locator('html[data-hydrated="true"]').waitFor({ timeout: 15_000 });
}

async function settle(page: Page) {
	await page.waitForLoadState('networkidle').catch(() => undefined);
	// Past the fakes' short delays, but before any "Still loading" timer on a page that never loads.
	await page.waitForTimeout(600);
	await page.evaluate(() => document.fonts.ready);
}

function fileName(url: string, width: number, suffix = '') {
	const slug = url.replace(/^\//, '').replace(/[^a-zA-Z0-9]+/g, '_') || 'root';
	return `${slug}${suffix}@${width}.png`;
}

async function snap(page: Page, outDir: string, name: string) {
	await page.screenshot({
		path: join(outDir, name),
		fullPage: true,
		animations: 'disabled',
		caret: 'hide'
	});
}

/** Interactive states that are not reachable by URL: overlays and a validation error. */
async function states(page: Page, outDir: string, width: number) {
	const tries: Array<[string, string, (p: Page) => Promise<void>]> = [
		[
			'/teams/members',
			'account-menu',
			async (p) => {
				await p.getByRole('button', { name: /^You/ }).locator('visible=true').first().click();
			}
		],
		[
			'/work',
			'new-menu',
			async (p) => {
				await p.getByRole('button', { name: /^New/ }).first().click();
			}
		],
		[
			'/create',
			'create-invalid',
			async (p) => {
				await p.getByRole('button', { name: 'Create organization' }).click();
			}
		],
		['/sign-in', 'sign-in', async () => {}]
	];
	if (width < 640)
		tries.unshift([
			'/home',
			'nav-drawer',
			async (p) => {
				await p.getByRole('button', { name: 'Menu', exact: true }).click();
			}
		]);
	for (const [url, label, act] of tries) {
		try {
			await visit(page, url);
			await settle(page);
			await act(page);
			await settle(page);
			await snap(page, outDir, fileName(url, width, `--${label}`));
		} catch (error) {
			console.log(`state ${label}@${width} skipped: ${(error as Error).message.split('\n')[0]}`);
		}
	}
}

async function capture(outDir: string, port: number, urlsFile?: string) {
	const base = `http://localhost:${port}`;
	await mkdir(outDir, { recursive: true });
	const browser = await chromium.launch();
	let urls: string[];
	if (urlsFile) urls = JSON.parse(await readFile(urlsFile, 'utf8'));
	else {
		// Crawl from the seeds at the wide width: keep every seed, and add linked pages up to two per route pattern.
		const patterns = await routePatterns();
		const perPattern = new Map<string, number>();
		const page = await signedInPage(browser, base, 1440);
		const seen = new Set<string>(seeds);
		const queue = [...seeds];
		for (let i = 0; i < queue.length && queue.length < 140; i++) {
			try {
				await visit(page, queue[i]);
				await settle(page);
			} catch {
				continue;
			}
			const hrefs = await page.$$eval('a[href^="/"]', (as) =>
				as.map((a) => a.getAttribute('href') ?? '')
			);
			for (const href of hrefs) {
				const clean = href.split('#')[0];
				const path = clean.split('?')[0];
				const pattern = patterns.find((p) => p.test(path));
				if (!pattern || seen.has(clean) || /sign-out|\/sessions/.test(clean)) continue;
				const n = perPattern.get(pattern.source) ?? 0;
				if (n >= 2) continue;
				perPattern.set(pattern.source, n + 1);
				seen.add(clean);
				queue.push(clean);
			}
		}
		await page.context().close();
		urls = queue;
		await writeFile(join(outDir, 'urls.json'), JSON.stringify(urls, null, '\t'));
	}
	for (const width of widths) {
		const page = await signedInPage(browser, base, width);
		for (const url of urls) {
			try {
				await visit(page, url);
				await settle(page);
				await snap(page, outDir, fileName(url, width));
			} catch (error) {
				console.log(`${url}@${width} skipped: ${(error as Error).message.split('\n')[0]}`);
			}
		}
		await states(page, outDir, width);
		await page.context().close();
	}
	await browser.close();
	console.log(
		`captured ${(await readdir(outDir)).filter((f) => f.endsWith('.png')).length} screenshots in ${outDir}`
	);
}

async function diff(beforeDir: string, afterDir: string, report?: string) {
	const before = (await readdir(beforeDir)).filter((f) => f.endsWith('.png')).sort();
	const after = new Set((await readdir(afterDir)).filter((f) => f.endsWith('.png')));
	const rows: string[] = [];
	let identical = 0;
	for (const name of before) {
		if (!after.has(name)) {
			rows.push(`| ${name} | missing after | |`);
			continue;
		}
		after.delete(name);
		const a = PNG.sync.read(await readFile(join(beforeDir, name)));
		const b = PNG.sync.read(await readFile(join(afterDir, name)));
		if (a.width !== b.width || a.height !== b.height) {
			rows.push(`| ${name} | size ${a.width}x${a.height} → ${b.width}x${b.height} | |`);
			continue;
		}
		let changed = 0;
		let maxDelta = 0;
		for (let i = 0; i < a.data.length; i += 4) {
			const d = Math.max(
				Math.abs(a.data[i] - b.data[i]),
				Math.abs(a.data[i + 1] - b.data[i + 1]),
				Math.abs(a.data[i + 2] - b.data[i + 2]),
				Math.abs(a.data[i + 3] - b.data[i + 3])
			);
			if (d > 0) {
				changed++;
				if (d > maxDelta) maxDelta = d;
			}
		}
		if (changed === 0) identical++;
		else rows.push(`| ${name} | ${changed} px differ | max channel delta ${maxDelta} |`);
	}
	for (const name of after) rows.push(`| ${name} | new after | |`);
	const text = [
		`# Look tokens pixel diff`,
		'',
		`${before.length} screenshots before, ${identical} identical after.`,
		'',
		rows.length
			? '| Screenshot | Result | Detail |\n| --- | --- | --- |\n' + rows.join('\n')
			: 'No differences.',
		''
	].join('\n');
	if (report) await writeFile(report, text);
	console.log(text);
	if (rows.length) process.exitCode = 1;
}

const [mode, ...args] = process.argv.slice(2);
const flag = (name: string) => {
	const i = args.indexOf(name);
	return i >= 0 ? args[i + 1] : undefined;
};
if (mode === 'capture' && args[0])
	await capture(args[0], Number(flag('--port') ?? 5364), flag('--urls'));
else if (mode === 'diff' && args[0] && args[1]) await diff(args[0], args[1], args[2]);
else {
	console.error(
		'usage: bun scripts/look-tokens-shots.ts capture <outDir> [--port 5364] [--urls <urls.json>] | diff <beforeDir> <afterDir> [<report.md>]'
	);
	process.exitCode = 2;
}
