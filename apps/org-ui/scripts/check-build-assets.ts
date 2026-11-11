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
		else if (entry.isFile() && /\.(?:css|m?js)$/i.test(entry.name)) {
			const stylesheet = /\.css$/i.test(entry.name);
			if (stylesheet) css++; else js++;
			const text = await readFile(file, 'utf8');
			const hasData = stylesheet
				? /url\(\s*['"]?\s*data:/i.test(text)
				: /['"`]data:[\w.+-]*\/?[\w.+-]*[;,]/i.test(text);
			if (hasData) problems.push(`${relative(directory, file)}: ${stylesheet ? 'CSS data URL' : 'JS data URI'}`);
		}
	}
}

try {
	await scan(directory);
	if (problems.length) throw new Error(problems.join('\n'));
	console.log(`Build assets checked: ${css} CSS and ${js} JS; no data URIs.`);
} catch (error) {
	console.error(error);
	process.exitCode = 1;
}
