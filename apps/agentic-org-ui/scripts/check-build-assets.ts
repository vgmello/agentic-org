import { existsSync } from 'node:fs';
import { readdir, readFile } from 'node:fs/promises';
import { join, relative } from 'node:path';
import { fileURLToPath } from 'node:url';
import { gzipSync } from 'node:zlib';

const SHELL_GZIP_BUDGET_KB = 280;
const defaultDir = fileURLToPath(new URL('../build/', import.meta.url));
const directory = process.argv[2] ?? defaultDir;
const checkBudgets = !process.argv[2];
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

type ViteManifest = Record<string, { file?: string; src?: string; imports?: string[] }>;

async function resolveLayoutNodeKeys(
	manifest: ViteManifest
): Promise<{ rootLayout?: string; shellLayout?: string }> {
	const keys = Object.keys(manifest);
	let rootLayout = keys.find((k) => k.endsWith('routes/+layout.svelte'));
	let shellLayout = keys.find((k) => k.endsWith('routes/(shell)/+layout.svelte'));
	const nodesDir = fileURLToPath(
		new URL('../.svelte-kit/generated/client/nodes/', import.meta.url)
	);
	if ((rootLayout && shellLayout) || !existsSync(nodesDir)) return { rootLayout, shellLayout };
	const entries = await readdir(nodesDir);
	const files = await Promise.all(
		entries.map(async (entry) => ({ entry, text: await readFile(join(nodesDir, entry), 'utf8') }))
	);
	for (const { entry, text } of files) {
		if (!rootLayout && text.includes('routes/+layout.svelte')) {
			rootLayout = keys.find((k) => k.endsWith(`nodes/${entry}`));
		}
		if (!shellLayout && text.includes('routes/(shell)/+layout.svelte')) {
			shellLayout = keys.find((k) => k.endsWith(`nodes/${entry}`));
		}
	}
	return { rootLayout, shellLayout };
}

function collectJsClosure(manifest: ViteManifest, entryKeys: string[]): string[] {
	const visited = new Set<string>();
	const stack = [...entryKeys];
	while (stack.length > 0) {
		const key = stack.pop()!;
		if (visited.has(key) || !manifest[key]) continue;
		visited.add(key);
		for (const imp of manifest[key].imports ?? []) stack.push(imp);
	}
	return [...visited]
		.map((k) => manifest[k]?.file)
		.filter((f): f is string => Boolean(f && f.endsWith('.js')));
}

async function checkShellBundleBudget(): Promise<string> {
	const clientRoot = fileURLToPath(new URL('../.svelte-kit/output/client/', import.meta.url));
	const manifestPath = join(clientRoot, '.vite/manifest.json');
	if (!existsSync(manifestPath)) return '';
	const manifest = JSON.parse(await readFile(manifestPath, 'utf8')) as ViteManifest;
	const appEntry = Object.keys(manifest).find(
		(k) => k.endsWith('client/entry.js') || k.includes('entry/start.js')
	);
	const { rootLayout, shellLayout } = await resolveLayoutNodeKeys(manifest);
	if (!shellLayout) {
		problems.push(
			'Could not locate SvelteKit shell layout manifest entry for bundle budget check.'
		);
		return '';
	}
	const entryKeys = [appEntry, rootLayout, shellLayout].filter((x): x is string => Boolean(x));
	const jsFiles = collectJsClosure(manifest, entryKeys);
	const buffers = await Promise.all(jsFiles.map((f) => readFile(join(clientRoot, f))));
	const gzBytes = buffers.reduce((sum, buf) => sum + gzipSync(buf).length, 0);
	const gzKb = +(gzBytes / 1024).toFixed(1);
	if (gzKb > SHELL_GZIP_BUDGET_KB) {
		problems.push(
			`Shell first-paint JS closure is ${gzKb} kB gzip (budget <= ${SHELL_GZIP_BUDGET_KB} kB; see .agents/rules/svelte-architecture-and-quality.md §2)`
		);
	}
	return ` Shell first-paint JS: ${gzKb} kB gzip (budget <= ${SHELL_GZIP_BUDGET_KB} kB).`;
}

try {
	await scan(directory);
	const budgetNote = checkBudgets ? await checkShellBundleBudget() : '';
	if (problems.length) throw new Error(problems.join('\n'));
	console.log(
		`Build assets checked: ${css} CSS and ${js} JS; no data URIs, test CSP tooling or auth fixtures.${budgetNote}`
	);
} catch (error) {
	console.error(error);
	process.exitCode = 1;
}
