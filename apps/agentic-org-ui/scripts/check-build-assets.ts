import { readdir, readFile } from 'node:fs/promises';
import { join, relative } from 'node:path';
import { fileURLToPath } from 'node:url';

const directory = process.argv[2] ?? fileURLToPath(new URL('../build/', import.meta.url));
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
			if (stylesheet) css++; else if (javascript) js++;
			const text = await readFile(file, 'utf8');
			if (/report-uri|report-sample|\/__csp-(?:report|status|clear)|org-test-csp-policy|e2e-csp\/vite-policy/i.test(text)) {
				problems.push(`${relative(directory, file)}: test CSP tooling`);
			}
			const hasData = stylesheet
				? /url\(\s*['"]?\s*data:/i.test(text)
				: javascript && /['"`]data:[\w.+-]*\/?[\w.+-]*[;,]/i.test(text);
			if (hasData) problems.push(`${relative(directory, file)}: ${stylesheet ? 'CSS data URL' : 'JS data URI'}`);
		}
	}
}

try {
	await scan(directory);
	if (problems.length) throw new Error(problems.join('\n'));
	console.log(`Build assets checked: ${css} CSS and ${js} JS; no data URIs or test CSP tooling.`);
} catch (error) {
	console.error(error);
	process.exitCode = 1;
}
