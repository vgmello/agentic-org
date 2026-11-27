import { existsSync } from 'node:fs';
import { readdir, readFile } from 'node:fs/promises';
import { join, relative } from 'node:path';
import { fileURLToPath } from 'node:url';
import { gzipSync } from 'node:zlib';

interface BundleBudget {
	shellJsGzipKb: number;
	shellCssGzipKb: number;
	shellTotalGzipKb: number;
	onboardingShellJsGzipKb: number;
	onboardingShellCssGzipKb: number;
	onboardingShellTotalGzipKb: number;
	nestedLayoutMaxJsGzipKb: number;
	nestedLayoutOverrides: Record<string, number>;
	routeMaxJsGzipKb: number;
	routeOverrides: Record<string, number>;
	onboardingRouteMaxJsGzipKb: number;
	onboardingRouteOverrides: Record<string, number>;
}

const args = process.argv.slice(2);
const skipBudgets = args.includes('--skip-budgets');
const dirArg = args.find((a) => !a.startsWith('--'));
const defaultDir = fileURLToPath(new URL('../build/', import.meta.url));
const directory = dirArg ?? defaultDir;
const budgetPath = fileURLToPath(new URL('../bundle-budget.json', import.meta.url));
const problems: string[] = [];
let css = 0;
let js = 0;

async function scan(path: string) {
	for (const entry of await readdir(path, { withFileTypes: true })) {
		const file = join(path, entry.name);
		if (entry.isDirectory()) await scan(file);
		else if (entry.isFile() && /\.(?:html|css|m?js)$/i.test(entry.name)) {
			const stylesheet = /\.css$/i.test(entry.name);
			const javascript = /\.m?js$/i.test(entry.name);
			if (stylesheet) css++;
			else if (javascript) js++;
			const text = await readFile(file, 'utf8');
			if (
				/report-uri|report-sample|\/__csp-(?:report|status|clear)|org-test-csp-policy|e2e-csp\/vite-policy/i.test(
					text
				)
			) {
				problems.push(`${relative(directory, file)}: test CSP tooling`);
			}
			if (/\/__p6-auth\/|sessionTestMiddleware|p6_auth_case|session-browser-harness/.test(text)) {
				problems.push(`${relative(directory, file)}: test auth fixture`);
			}
			const hasData = stylesheet
				? /url\(\s*['"]?\s*data:/i.test(text)
				: javascript && /['"`]data:[\w.+-]*\/?[\w.+-]*[;,]/i.test(text);
			if (hasData)
				problems.push(
					`${relative(directory, file)}: ${stylesheet ? 'CSS data URL' : 'JS data URI'}`
				);
		}
	}
}

type ViteManifest = Record<
	string,
	{ file?: string; src?: string; imports?: string[]; css?: string[] }
>;

async function mapRouteNodes(manifest: ViteManifest): Promise<Map<string, string>> {
	const keys = Object.keys(manifest);
	const nodesDir = fileURLToPath(
		new URL('../.svelte-kit/generated/client/nodes/', import.meta.url)
	);
	if (!existsSync(nodesDir)) {
		problems.push('Missing .svelte-kit/generated/client/nodes directory for bundle budget check.');
		return new Map();
	}
	const entries = await readdir(nodesDir);
	const files = await Promise.all(
		entries.map(async (entry) => ({ entry, text: await readFile(join(nodesDir, entry), 'utf8') }))
	);
	const nodeMap = new Map<string, string>();
	for (const { entry, text } of files) {
		const m = /from\s+["']([^"']+\.svelte)["']/.exec(text);
		const key = keys.find((k) => k.endsWith(`nodes/${entry}`));
		if (m && key) {
			nodeMap.set(m[1].replace(/^(?:\.\.\/)+/, ''), key);
		}
	}
	return nodeMap;
}

function collectClosure(
	manifest: ViteManifest,
	entryKeys: string[]
): { js: string[]; css: string[] } {
	const visited = new Set<string>();
	const stack = [...entryKeys];
	while (stack.length > 0) {
		const key = stack.pop()!;
		if (visited.has(key) || !manifest[key]) continue;
		visited.add(key);
		for (const imp of manifest[key].imports ?? []) stack.push(imp);
	}
	const jsFiles = new Set<string>();
	const cssFiles = new Set<string>();
	for (const key of visited) {
		const entry = manifest[key];
		if (entry?.file?.endsWith('.js')) jsFiles.add(entry.file);
		for (const c of entry?.css ?? []) cssFiles.add(c);
	}
	return { js: [...jsFiles], css: [...cssFiles] };
}

async function gzipKb(clientRoot: string, files: string[]): Promise<number> {
	const buffers = await Promise.all(files.map((f) => readFile(join(clientRoot, f))));
	const bytes = buffers.reduce((sum, buf) => sum + gzipSync(buf).length, 0);
	return +(bytes / 1024).toFixed(1);
}

async function checkClosureBudget(
	label: string,
	clientRoot: string,
	manifest: ViteManifest,
	entryKeys: string[],
	limits: { js: number; css: number; total: number }
): Promise<{ jsFiles: Set<string>; jsKb: number; cssKb: number; totalKb: number }> {
	const closure = collectClosure(manifest, entryKeys);
	const jsKb = await gzipKb(clientRoot, closure.js);
	const cssKb = await gzipKb(clientRoot, closure.css);
	const totalKb = +(jsKb + cssKb).toFixed(1);
	if (jsKb > limits.js) problems.push(`${label} JS ${jsKb} kB > budget ${limits.js} kB gzip.`);
	if (cssKb > limits.css) problems.push(`${label} CSS ${cssKb} kB > budget ${limits.css} kB gzip.`);
	if (totalKb > limits.total) {
		problems.push(`${label} JS+CSS ${totalKb} kB > budget ${limits.total} kB gzip.`);
	}
	return { jsFiles: new Set(closure.js), jsKb, cssKb, totalKb };
}

async function checkIncrementalGroup(options: {
	label: string;
	clientRoot: string;
	manifest: ViteManifest;
	nodeMap: Map<string, string>;
	prefix: string;
	suffix: string;
	excludeExact?: string;
	baseJsSet: Set<string>;
	defaultLimit: number;
	overrides: Record<string, number>;
}): Promise<number> {
	const seen = new Set<string>();
	for (const [comp, key] of options.nodeMap.entries()) {
		if (!comp.startsWith(options.prefix) || !comp.endsWith(options.suffix)) continue;
		if (comp === options.excludeExact) continue;
		const routeName = comp.slice(options.prefix.length);
		seen.add(routeName);
		const closure = collectClosure(options.manifest, [key]);
		const incJs = closure.js.filter((f) => !options.baseJsSet.has(f));
		const incJsKb = await gzipKb(options.clientRoot, incJs);
		const limit = options.overrides[routeName] ?? options.defaultLimit;
		if (incJsKb > limit) {
			problems.push(
				`${options.label} "${routeName}" incremental JS is ${incJsKb} kB > budget ${limit} kB gzip.`
			);
		}
	}
	if (seen.size === 0) {
		problems.push(`Zero ${options.label} entries discovered in manifest (fail-closed).`);
	}
	for (const overrideKey of Object.keys(options.overrides)) {
		if (!seen.has(overrideKey)) {
			problems.push(`Stale ${options.label} override "${overrideKey}" in bundle-budget.json.`);
		}
	}
	return seen.size;
}

async function checkBundleBudgets(): Promise<string> {
	if (!existsSync(budgetPath)) {
		problems.push('Missing apps/agentic-org-ui/bundle-budget.json.');
		return '';
	}
	const budget = JSON.parse(await readFile(budgetPath, 'utf8')) as BundleBudget;

	const clientRoot = fileURLToPath(new URL('../.svelte-kit/output/client/', import.meta.url));
	const manifestPath = join(clientRoot, '.vite/manifest.json');
	if (!existsSync(manifestPath)) {
		problems.push(
			`Missing Vite client manifest at ${manifestPath}; cannot verify bundle budgets (fail-closed).`
		);
		return '';
	}

	const manifest = JSON.parse(await readFile(manifestPath, 'utf8')) as ViteManifest;
	const keys = Object.keys(manifest);
	const startEntry = keys.find(
		(k) => k.endsWith('client/entry.js') || k.includes('entry/start.js')
	);
	const appEntry = keys.find(
		(k) => k.endsWith('client-optimized/app.js') || k.endsWith('client/app.js')
	);
	const nodeMap = await mapRouteNodes(manifest);
	const rootLayout = nodeMap.get('src/routes/+layout.svelte');
	const shellLayout = nodeMap.get('src/routes/(shell)/+layout.svelte');
	const onboardingLayout = nodeMap.get('src/routes/(onboarding)/+layout.svelte');

	if (!startEntry || !appEntry || !rootLayout || !shellLayout || !onboardingLayout) {
		problems.push(
			`Could not locate required manifest entries (start=${Boolean(startEntry)}, app=${Boolean(appEntry)}, root=${Boolean(rootLayout)}, shell=${Boolean(shellLayout)}, onboarding=${Boolean(onboardingLayout)}).`
		);
		return '';
	}

	const shell = await checkClosureBudget(
		'Shell first-paint',
		clientRoot,
		manifest,
		[startEntry, appEntry, rootLayout, shellLayout],
		{ js: budget.shellJsGzipKb, css: budget.shellCssGzipKb, total: budget.shellTotalGzipKb }
	);
	const onboarding = await checkClosureBudget(
		'Onboarding first-paint',
		clientRoot,
		manifest,
		[startEntry, appEntry, rootLayout, onboardingLayout],
		{
			js: budget.onboardingShellJsGzipKb,
			css: budget.onboardingShellCssGzipKb,
			total: budget.onboardingShellTotalGzipKb
		}
	);

	const nestedCount = await checkIncrementalGroup({
		label: 'Nested (shell) layout',
		clientRoot,
		manifest,
		nodeMap,
		prefix: 'src/routes/(shell)/',
		suffix: '+layout.svelte',
		excludeExact: 'src/routes/(shell)/+layout.svelte',
		baseJsSet: shell.jsFiles,
		defaultLimit: budget.nestedLayoutMaxJsGzipKb,
		overrides: budget.nestedLayoutOverrides
	});
	const shellRoutes = await checkIncrementalGroup({
		label: '(shell) route',
		clientRoot,
		manifest,
		nodeMap,
		prefix: 'src/routes/(shell)/',
		suffix: '+page.svelte',
		baseJsSet: shell.jsFiles,
		defaultLimit: budget.routeMaxJsGzipKb,
		overrides: budget.routeOverrides
	});
	const onboardingRoutes = await checkIncrementalGroup({
		label: '(onboarding) route',
		clientRoot,
		manifest,
		nodeMap,
		prefix: 'src/routes/(onboarding)/',
		suffix: '+page.svelte',
		baseJsSet: onboarding.jsFiles,
		defaultLimit: budget.onboardingRouteMaxJsGzipKb,
		overrides: budget.onboardingRouteOverrides
	});

	return ` Shell: ${shell.jsKb}+${shell.cssKb}=${shell.totalKb} kB (<= ${budget.shellTotalGzipKb} kB); Onboarding: ${onboarding.jsKb}+${onboarding.cssKb}=${onboarding.totalKb} kB (<= ${budget.onboardingShellTotalGzipKb} kB); ${nestedCount} nested layouts, ${shellRoutes} shell routes, ${onboardingRoutes} onboarding routes checked.`;
}

try {
	await scan(directory);
	const budgetNote = skipBudgets ? '' : await checkBundleBudgets();
	if (problems.length) throw new Error(problems.join('\n'));
	console.log(
		`Build assets checked: ${css} CSS and ${js} JS; no data URIs, test CSP tooling or auth fixtures.${budgetNote}`
	);
} catch (error) {
	console.error(error);
	process.exitCode = 1;
}
