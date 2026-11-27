import { existsSync } from 'node:fs';
import { readdir, readFile } from 'node:fs/promises';
import { join, relative } from 'node:path';
import { fileURLToPath } from 'node:url';
import { gzipSync } from 'node:zlib';

interface BundleBudget {
	shellJsGzipKb: number;
	shellCssGzipKb: number;
	shellTotalGzipKb: number;
	routeMaxJsGzipKb: number;
	routeOverrides: Record<string, number>;
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

async function checkRouteBudgets(
	clientRoot: string,
	manifest: ViteManifest,
	nodeMap: Map<string, string>,
	shellJsSet: Set<string>,
	budget: BundleBudget
): Promise<number> {
	let checkedRoutes = 0;
	for (const [comp, key] of nodeMap.entries()) {
		if (!comp.startsWith('src/routes/(shell)/') || !comp.endsWith('+page.svelte')) continue;
		checkedRoutes++;
		const routeName = comp.slice('src/routes/(shell)/'.length);
		const closure = collectClosure(manifest, [key]);
		const incJs = closure.js.filter((f) => !shellJsSet.has(f));
		const incJsKb = await gzipKb(clientRoot, incJs);
		const limit = budget.routeOverrides[routeName] ?? budget.routeMaxJsGzipKb;
		if (incJsKb > limit) {
			problems.push(
				`Route "${routeName}" incremental JS is ${incJsKb} kB > budget ${limit} kB gzip.`
			);
		}
	}
	if (checkedRoutes === 0) {
		problems.push('Zero (shell) page routes discovered in manifest (fail-closed).');
	}
	return checkedRoutes;
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
	const appEntry = keys.find((k) => k.endsWith('client/entry.js') || k.includes('entry/start.js'));
	const nodeMap = await mapRouteNodes(manifest);
	const rootLayout = nodeMap.get('src/routes/+layout.svelte');
	const shellLayout = nodeMap.get('src/routes/(shell)/+layout.svelte');

	if (!appEntry || !rootLayout || !shellLayout) {
		problems.push(
			`Could not locate required manifest entries (appEntry=${Boolean(appEntry)}, rootLayout=${Boolean(rootLayout)}, shellLayout=${Boolean(shellLayout)}).`
		);
		return '';
	}

	const shell = collectClosure(manifest, [appEntry, rootLayout, shellLayout]);
	const shellJsKb = await gzipKb(clientRoot, shell.js);
	const shellCssKb = await gzipKb(clientRoot, shell.css);
	const shellTotalKb = +(shellJsKb + shellCssKb).toFixed(1);

	if (shellJsKb > budget.shellJsGzipKb) {
		problems.push(`Shell first-paint JS ${shellJsKb} kB > budget ${budget.shellJsGzipKb} kB gzip.`);
	}
	if (shellCssKb > budget.shellCssGzipKb) {
		problems.push(`Shell first-paint CSS ${shellCssKb} kB > budget ${budget.shellCssGzipKb} kB gzip.`);
	}
	if (shellTotalKb > budget.shellTotalGzipKb) {
		problems.push(
			`Shell first-paint JS+CSS ${shellTotalKb} kB > budget ${budget.shellTotalGzipKb} kB gzip.`
		);
	}

	const checkedRoutes = await checkRouteBudgets(
		clientRoot,
		manifest,
		nodeMap,
		new Set(shell.js),
		budget
	);

	return ` Shell first-paint: ${shellJsKb} kB JS + ${shellCssKb} kB CSS = ${shellTotalKb} kB gzip (budget <= ${budget.shellTotalGzipKb} kB); ${checkedRoutes} routes checked.`;
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
