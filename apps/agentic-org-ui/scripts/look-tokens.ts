// Rewrites raw Tailwind palette classes (text-gray-400, bg-white/5, outline-indigo-400, …) to the
// look-token utilities defined in src/app.css, using the table in
// src/lib/platform/tooling/look-tokens.ts. Rerunnable and idempotent: a second run changes nothing.
//
//   bun scripts/look-tokens.ts [--dry] [paths…]     (paths relative to apps/agentic-org-ui; default src)
//
// Rewrites .svelte, .ts, .js and .html files (app.html), including class: directives, cn()/clsx strings, ternaries
// and the shared class helpers; opacity modifiers and variants are kept. Prints a summary of
// every rewrite and each use it could not map (those keep their palette class and fail the guard).
//
// A legacy file listed in scripts/format-baseline.json (allowed to stay unformatted while
// unchanged) keeps that status when this rewrite is its only change: its baseline hash is moved
// to the rewritten content. A file that was prettier-clean before the rewrite is formatted after it
// (token names are longer, so a class attribute can pass the print width). Anything else is left
// for format:check to judge.
import { createHash } from 'node:crypto';
import { readFile, readdir, stat, writeFile } from 'node:fs/promises';
import { join, relative } from 'node:path';
import { fileURLToPath } from 'node:url';
import * as prettier from 'prettier';
import { allowlist, rewrite } from '../src/lib/platform/tooling/look-tokens';

process.chdir(fileURLToPath(new URL('../', import.meta.url)));
const baselinePath = 'scripts/format-baseline.json';
const baseline: Record<string, string> = JSON.parse(await readFile(baselinePath, 'utf8'));
const hash = (text: string) => `sha256:${createHash('sha256').update(text).digest('hex')}`;
let rebaselined = 0;
let reformatted = 0;

/** Formats `after` when `before` was prettier-clean, so the rewrite does not break format:check. */
async function keepFormatted(file: string, before: string, after: string) {
	const options = { ...(await prettier.resolveConfig(file)), filepath: file };
	if (!(await prettier.check(before, options))) return after;
	const formatted = await prettier.format(after, options);
	if (formatted !== after) reformatted++;
	return formatted;
}

const args = process.argv.slice(2);
const dry = args.includes('--dry');
const roots = args.filter((a) => !a.startsWith('--'));
if (roots.length === 0) roots.push('src');

async function files(path: string): Promise<string[]> {
	const info = await stat(path);
	if (info.isFile()) return [path];
	const out: string[] = [];
	for (const entry of await readdir(path, { withFileTypes: true })) {
		const child = join(path, entry.name);
		if (entry.isDirectory()) out.push(...(await files(child)));
		else if (/\.(svelte|ts|js|html)$/.test(entry.name)) out.push(child);
	}
	return out;
}

const tally = new Map<string, number>();
const unmapped: string[] = [];
let changedFiles = 0;
let rewrites = 0;
for (const root of roots) {
	for (const file of (await files(root)).sort()) {
		const name = relative(process.cwd(), file);
		if (allowlist[name]) continue;
		const before = await readFile(file, 'utf8');
		const result = rewrite(before, name);
		for (const u of result.unmapped) unmapped.push(`${name}:${u.line}  ${u.class}`);
		if (result.changes.length === 0) continue;
		changedFiles++;
		rewrites += result.changes.length;
		for (const c of result.changes)
			tally.set(`${c.from} → ${c.to}`, (tally.get(`${c.from} → ${c.to}`) ?? 0) + 1);
		if (dry) continue;
		if (baseline[name] === hash(before)) {
			await writeFile(file, result.text);
			baseline[name] = hash(result.text);
			rebaselined++;
		} else await writeFile(file, await keepFormatted(name, before, result.text));
	}
}
if (rebaselined) await writeFile(baselinePath, `${JSON.stringify(baseline, null, '\t')}\n`);

for (const [pair, count] of [...tally].sort((a, b) => b[1] - a[1]))
	console.log(`${String(count).padStart(5)}  ${pair}`);
console.log(
	`\n${dry ? 'Would rewrite' : 'Rewrote'} ${rewrites} classes (${tally.size} distinct) in ${changedFiles} files` +
		(rebaselined ? `; moved ${rebaselined} unchanged legacy files' format baseline` : '') +
		(reformatted ? `; reformatted ${reformatted} formatted files` : '') +
		'.'
);
if (unmapped.length) {
	console.log(
		`\nCould not map ${unmapped.length} uses (left as written; add a token or an override):`
	);
	for (const u of unmapped) console.log(`  ${u}`);
} else console.log('Nothing left unmapped.');
