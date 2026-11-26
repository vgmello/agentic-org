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

async function checkShellBundleBudget(): Promise<string> {
	const clientRoot = fileURLToPath(new URL('../.svelte-kit/output/client/', import.meta.url));
	const manifestPath = join(clientRoot, '.vite/manifest.json');
	if (!existsSync(manifestPath)) return '';
	const manifest = JSON.parse(await readFile(manifestPath, 'utf8')) as Record<
		string,
		{ file?: string; imports?: string[] }
	>;
	const entries = Object.keys(manifest).filter(
		(k) => k.includes('entry/start.js') || k.includes('nodes/0.js') || k.includes('nodes/3.js')
	);
	const visited = new Set<string>();
	const stack = [...entries];
	while (stack.length > 0) {
		const key = stack.pop()!;
		if (visited.has(key) || !manifest[key]) continue;
		visited.add(key);
		for (const imp of manifest[key].imports ?? []) stack.push(imp);
	}
	let gzBytes = 0;
	for (const key of visited) {
		const file = manifest[key]?.file;
		if (!file || !file.endsWith('.js')) continue;
		gzBytes += gzipSync(await readFile(join(clientRoot, file))).length;
	}
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
