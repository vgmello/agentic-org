import { execFileSync } from 'node:child_process';
import { existsSync, readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';
import { isNonProductionModule } from './production-import-guard';

const UI_ROOT = resolve(import.meta.dirname, '../../../..');
const ALLOW_PATH = resolve(UI_ROOT, 'large-files.allow');
const CAP = 700;
const TARGET = 500;

function countLines(filePath: string): number {
	const buf = readFileSync(filePath);
	if (buf.length === 0) return 0;
	let newlines = 0;
	for (let i = 0; i < buf.length; i++) {
		if (buf[i] === 10) newlines++;
	}
	return buf[buf.length - 1] === 10 ? newlines : newlines + 1;
}

function listTrackedProductionFiles(): string[] {
	const raw = execFileSync('git', ['ls-files', '-z', '--', 'src'], {
		cwd: UI_ROOT,
		encoding: 'utf8'
	});
	return raw
		.split('\0')
		.filter((rel) => rel.length > 0 && /\.(?:svelte|ts|js|css)$/.test(rel))
		.filter((rel) => !isNonProductionModule(`/${rel}`))
		.filter((rel) => existsSync(resolve(UI_ROOT, rel)))
		.sort();
}

function parseAllowlist(text: string): { entries: Map<string, number>; errors: string[] } {
	const entries = new Map<string, number>();
	const errors: string[] = [];
	const lines = text.split('\n');
	if (!lines[0]?.startsWith('#')) {
		errors.push('large-files.allow must start with a # comment header');
	}
	let prevPath = '';
	for (let i = 0; i < lines.length; i++) {
		const line = lines[i].trim();
		if (!line || line.startsWith('#')) continue;
		const match = /^(\S+)\s+(\d+)$/.exec(line);
		if (!match) {
			errors.push(`large-files.allow:${i + 1}: malformed entry "${line}"`);
			continue;
		}
		const [, filePath, countStr] = match;
		const count = Number(countStr);
		if (entries.has(filePath)) {
			errors.push(`large-files.allow:${i + 1}: duplicate path "${filePath}"`);
		}
		if (prevPath && filePath < prevPath) {
			errors.push(`large-files.allow:${i + 1}: unsorted path "${filePath}" after "${prevPath}"`);
		}
		prevPath = filePath;
		if (isNonProductionModule(`/${filePath}`)) {
			errors.push(`large-files.allow:${i + 1}: lists non-production file "${filePath}"`);
		}
		if (count <= CAP) {
			errors.push(`large-files.allow:${i + 1}: "${filePath}" (${count}) <= ${CAP}; delete entry`);
		}
		entries.set(filePath, count);
	}
	return { entries, errors };
}

describe('UI file-size ratchet (TASK-225, CAP=700, TARGET=500)', () => {
	it('keeps unlisted production files <= 700 lines and ensures listed files only shrink', () => {
		const files = listTrackedProductionFiles();
		const actualCounts = new Map<string, number>();
		for (const rel of files) {
			actualCounts.set(rel, countLines(resolve(UI_ROOT, rel)));
		}

		if (process.env.UI_FILE_SIZE_RATCHET_PRINT === '1') {
			const over = [...actualCounts.entries()]
				.filter(([, n]) => n > CAP)
				.sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0))
				.map(([p, n]) => `${p} ${n}`)
				.join('\n');
			console.log(over);
		}

		const { entries: allow, errors } = parseAllowlist(readFileSync(ALLOW_PATH, 'utf8'));

		for (const [rel, n] of actualCounts) {
			const max = allow.get(rel);
			if (max === undefined && n > CAP) {
				errors.push(`${rel}: ${n} > ${CAP}; split toward <= ${TARGET} (never add to allowlist)`);
			} else if (max !== undefined && n !== max) {
				const hint =
					n > max
						? `grew from ${max} to ${n}; extract a cohesive module first`
						: n <= CAP
							? `shrank to ${n} <= ${CAP}; delete its entry from large-files.allow`
							: `shrank from ${max} to ${n}; lower its entry in large-files.allow`;
				errors.push(`${rel}: ${hint}`);
			}
		}

		for (const rel of allow.keys()) {
			if (!actualCounts.has(rel)) {
				errors.push(`${rel}: file removed or untracked; delete its entry from large-files.allow`);
			}
		}

		expect(errors).toEqual([]);
	});

	it('never raises counts or adds unlisted paths relative to UI_FILE_SIZE_RATCHET_BASE', () => {
		const baseRef = process.env.UI_FILE_SIZE_RATCHET_BASE?.trim();
		if (!baseRef) return;

		let baseText: string;
		try {
			baseText = execFileSync('git', ['show', `${baseRef}:apps/agentic-org-ui/large-files.allow`], {
				cwd: UI_ROOT,
				encoding: 'utf8'
			});
		} catch {
			// Base predates introduction of apps/agentic-org-ui/large-files.allow.
			return;
		}

		const base = parseAllowlist(baseText).entries;
		const head = parseAllowlist(readFileSync(ALLOW_PATH, 'utf8')).entries;
		const errors: string[] = [];

		const removedDesc = [...base.entries()]
			.filter(([p]) => !head.has(p))
			.sort(([, a], [, b]) => b - a);
		const addedDesc = [...head.entries()]
			.filter(([p]) => !base.has(p))
			.sort(([, a], [, b]) => b - a);

		for (const [p, count] of head) {
			const prev = base.get(p);
			if (prev !== undefined && count > prev) {
				errors.push(`${p}: raised from ${prev} to ${count} (allowlist may only shrink)`);
			}
		}

		if (addedDesc.length > removedDesc.length) {
			errors.push(
				`large-files.allow added ${addedDesc.length} path(s) vs ${removedDesc.length} removed: ${addedDesc.map(([p]) => p).join(', ')}`
			);
		}
		for (let i = 0; i < Math.min(addedDesc.length, removedDesc.length); i++) {
			const [newPath, newCount] = addedDesc[i];
			const [oldPath, oldCount] = removedDesc[i];
			if (newCount > oldCount) {
				errors.push(
					`${newPath} (${newCount}): exceeds paired removed entry ${oldPath} (${oldCount})`
				);
			}
		}

		expect(errors).toEqual([]);
	});
});
