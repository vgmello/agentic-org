import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { lstat, readFile, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import * as prettier from 'prettier';

// Only unchanged legacy files may use the baseline. Every new or edited file must format cleanly.
const app = fileURLToPath(new URL('../', import.meta.url));
process.chdir(app);
const baselinePath = new URL('./format-baseline.json', import.meta.url);
const recording = process.argv.includes('--record-baseline');
const baseline: Record<string, string> = recording
	? {}
	: JSON.parse(await readFile(baselinePath, 'utf8'));
const files = execFileSync(
	'git',
	['ls-files', '-z', '--cached', '--others', '--exclude-standard'],
	{
		cwd: app,
		encoding: 'utf8'
	}
)
	.split('\0')
	.filter(Boolean);
let checked = 0;
let skipped = 0;
const failures: string[] = [];
const recorded: Record<string, string> = {};

for (const file of [...new Set(files)].sort()) {
	if (file === 'scripts/format-baseline.json') continue;
	const absolute = join(app, file);
	try {
		if (!(await lstat(absolute)).isFile()) continue;
	} catch (error) {
		if ((error as NodeJS.ErrnoException).code === 'ENOENT') continue; // Deleted tracked file.
		throw error;
	}
	const info = await prettier.getFileInfo(file, {
		ignorePath: ['.gitignore', '.prettierignore'],
		plugins: ['prettier-plugin-svelte']
	});
	if (info.ignored || !info.inferredParser) continue;
	const source = await readFile(absolute, 'utf8');
	const hash = createHash('sha256').update(source).digest('hex');
	if (!recording && baseline[file] === hash) {
		skipped++;
		continue;
	}
	const options = await prettier.resolveConfig(file);
	if (!(await prettier.check(source, { ...options, filepath: file }))) {
		if (recording) recorded[file] = hash;
		else failures.push(file);
	}
	checked++;
}

if (recording) {
	await writeFile(baselinePath, `${JSON.stringify(recorded, null, '\t')}\n`);
	console.log(
		`Recorded ${Object.keys(recorded).length} legacy files. Review baseline changes explicitly.`
	);
} else {
	for (const file of failures) console.error(`Formatting required: ${file}`);
	console.log(
		`Format: ${checked} checked, ${skipped} unchanged legacy files, ${failures.length} failures.`
	);
	if (failures.length) process.exitCode = 1;
}
