import { execFileSync } from 'node:child_process';
import { existsSync, readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';
import { isNonProductionModule } from './production-import-guard';

const UI_ROOT = resolve(import.meta.dirname, '../../../..');
const ALLOW_PATH = resolve(UI_ROOT, 'suppressions.allow');

interface FileCounts {
	eslint: number;
	svelte: number;
	effect: number;
}

const ESLINT_DISABLE_LINE_RE = /(?:\/\/|\/\*|<!--)\s*eslint-disable(?:-next-line|-line)?\b([^\n]*)/g;
const SVELTE_IGNORE_LINE_RE = /(?:\/\/|\/\*|<!--)\s*svelte-ignore\b([^\n]*)/g;
const EFFECT_CALL_RE = /\$effect(?:\.pre)?\s*\(/g;

function hasEslintReason(tail: string): boolean {
	const cleaned = tail.replace(/(?:\*\/|-->)\s*$/, '').trim();
	const idx = cleaned.indexOf('--');
	return idx >= 0 && cleaned.slice(idx + 2).trim().length >= 3;
}

function hasSvelteIgnoreReason(lines: readonly string[], lineIndex: number): boolean {
	const line = lines[lineIndex] ?? '';
	const before = line.slice(0, line.indexOf('svelte-ignore'));
	if (/<!--\s*[^-][\s\S]{2,}-->/.test(before) || /\/\*\s*[\s\S]{3,}\*\//.test(before)) {
		return true;
	}
	for (let i = lineIndex - 1; i >= 0; i--) {
		const prev = lines[i].trim();
		if (!prev) return false;
		if (prev.includes('svelte-ignore')) continue;
		return /^(?:\/\/|\/\*|<!--)/.test(prev) && prev.replace(/^[/<!\s*-]+|[-/>\s*]+$/g, '').length >= 3;
	}
	return false;
}

function scanFile(absPath: string, rel: string): { counts: FileCounts; missingReason: string[] } {
	const source = readFileSync(absPath, 'utf8');
	const missingReason: string[] = [];
	let eslint = 0;
	let svelte = 0;

	const lines = source.split('\n');
	for (let i = 0; i < lines.length; i++) {
		const line = lines[i];
		for (const m of line.matchAll(ESLINT_DISABLE_LINE_RE)) {
			eslint++;
			if (!hasEslintReason(m[1] ?? '')) {
				missingReason.push(`${rel}:${i + 1}: eslint-disable must include "-- <reason>"`);
			}
		}
		for (const _m of line.matchAll(SVELTE_IGNORE_LINE_RE)) {
			void _m;
			svelte++;
			if (!hasSvelteIgnoreReason(lines, i)) {
				missingReason.push(
					`${rel}:${i + 1}: svelte-ignore must have a reason comment immediately before it`
				);
			}
		}
	}

	const effect = (source.match(EFFECT_CALL_RE) ?? []).length;
	return { counts: { eslint, svelte, effect }, missingReason };
}

function parseAllow(text: string): { entries: Map<string, FileCounts>; errors: string[] } {
	const entries = new Map<string, FileCounts>();
	const errors: string[] = [];
	const lines = text.split('\n');
	let prev = '';
	for (let i = 0; i < lines.length; i++) {
		const line = lines[i].trim();
		if (!line || line.startsWith('#')) continue;
		const m = /^(\S+)\s+eslint=(\d+)\s+svelte=(\d+)\s+effect=(\d+)$/.exec(line);
		if (!m) {
			errors.push(`suppressions.allow:${i + 1}: malformed entry "${line}"`);
			continue;
		}
		const [, path, e, s, ef] = m;
		if (entries.has(path)) errors.push(`suppressions.allow:${i + 1}: duplicate "${path}"`);
		if (prev && path < prev) errors.push(`suppressions.allow:${i + 1}: unsorted "${path}"`);
		prev = path;
		const counts = { eslint: Number(e), svelte: Number(s), effect: Number(ef) };
		if (counts.eslint === 0 && counts.svelte === 0 && counts.effect === 0) {
			errors.push(`suppressions.allow:${i + 1}: zero counts for "${path}"; delete entry`);
		}
		entries.set(path, counts);
	}
	return { entries, errors };
}

function resolveBaseRef(): string | undefined {
	const explicit = process.env.UI_FILE_SIZE_RATCHET_BASE?.trim();
	if (explicit) return explicit;
	try {
		return execFileSync('git', ['merge-base', 'HEAD', 'origin/v1'], {
			cwd: UI_ROOT,
			encoding: 'utf8',
			stdio: ['ignore', 'pipe', 'ignore']
		}).trim();
	} catch {
		return undefined;
	}
}

describe('UI suppressions & $effect ratchet', () => {
	it('requires -- <reason> on every eslint-disable / svelte-ignore and locks counts shrink-only', () => {
		const tracked = execFileSync('git', ['ls-files', '-z', '--', 'src'], {
			cwd: UI_ROOT,
			encoding: 'utf8'
		})
			.split('\0')
			.filter((rel) => rel.length > 0 && /\.(?:svelte|ts|js)$/.test(rel))
			.filter((rel) => !isNonProductionModule(`/${rel}`))
			.filter((rel) => existsSync(resolve(UI_ROOT, rel)))
			.sort();

		const actual = new Map<string, FileCounts>();
		const reasonErrors: string[] = [];

		for (const rel of tracked) {
			const { counts, missingReason } = scanFile(resolve(UI_ROOT, rel), rel);
			reasonErrors.push(...missingReason);
			if (counts.eslint > 0 || counts.svelte > 0 || counts.effect > 0) {
				actual.set(rel, counts);
			}
		}

		if (process.env.UI_SUPPRESSIONS_PRINT === '1') {
			const lines = [...actual.entries()]
				.sort(([a], [b]) => a.localeCompare(b))
				.map(([p, c]) => `${p} eslint=${c.eslint} svelte=${c.svelte} effect=${c.effect}`);
			console.log(lines.join('\n'));
		}

		const errors: string[] = [...reasonErrors];
		if (!existsSync(ALLOW_PATH)) {
			errors.push('Missing apps/agentic-org-ui/suppressions.allow');
			expect(errors).toEqual([]);
			return;
		}

		const { entries: allow, errors: parseErrors } = parseAllow(readFileSync(ALLOW_PATH, 'utf8'));
		errors.push(...parseErrors);

		for (const [rel, c] of actual) {
			const max = allow.get(rel);
			if (!max) {
				errors.push(
					`${rel}: unlisted suppressions/effects (eslint=${c.eslint} svelte=${c.svelte} effect=${c.effect}); prefer $derived/event handlers or fix root cause`
				);
				continue;
			}
			for (const key of ['eslint', 'svelte', 'effect'] as const) {
				if (c[key] > max[key]) {
					errors.push(`${rel}: ${key} grew from ${max[key]} to ${c[key]}`);
				} else if (c[key] < max[key]) {
					errors.push(`${rel}: ${key} shrank from ${max[key]} to ${c[key]}; lower suppressions.allow`);
				}
			}
		}

		for (const rel of allow.keys()) {
			if (!actual.has(rel)) {
				errors.push(`${rel}: zero suppressions/effects left; delete entry from suppressions.allow`);
			}
		}

		// Shrink-only check against base
		const baseRef = resolveBaseRef();
		if (baseRef) {
			try {
				const baseText = execFileSync(
					'git',
					['show', `${baseRef}:apps/agentic-org-ui/suppressions.allow`],
					{ cwd: UI_ROOT, encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] }
				);
				const base = parseAllow(baseText).entries;
				for (const [rel, c] of allow) {
					const prev = base.get(rel);
					if (!prev) {
						errors.push(`${rel}: newly added to suppressions.allow (allowlist may only shrink)`);
						continue;
					}
					for (const key of ['eslint', 'svelte', 'effect'] as const) {
						if (c[key] > prev[key]) {
							errors.push(`${rel}: ${key} raised from ${prev[key]} to ${c[key]} vs base`);
						}
					}
				}
			} catch {
				// Base predates suppressions.allow.
			}
		}

		expect(errors).toEqual([]);
	});
});
