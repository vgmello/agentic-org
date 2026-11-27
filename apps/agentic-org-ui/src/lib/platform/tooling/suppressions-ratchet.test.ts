import { execFileSync } from 'node:child_process';
import { existsSync, readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';
import { isNonProductionModule } from './production-import-guard';
import { scanSuppressionSource, type SuppressionCounts } from './suppressions-ratchet';

const UI_ROOT = resolve(import.meta.dirname, '../../../..');
const ALLOW_PATH = resolve(UI_ROOT, 'suppressions.allow');

function parseAllow(text: string): { entries: Map<string, SuppressionCounts>; errors: string[] } {
	const entries = new Map<string, SuppressionCounts>();
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
	it('bans inline ESLint configs and block eslint-disable, requires reasons, and locks counts shrink-only', () => {
		const files = execFileSync(
			'git',
			['ls-files', '--cached', '--others', '--exclude-standard', '-z', '--', 'src'],
			{
				cwd: UI_ROOT,
				encoding: 'utf8'
			}
		)
			.split('\0')
			.filter((rel) => rel.length > 0 && /\.(?:svelte|ts|js)$/.test(rel))
			.filter((rel) => !isNonProductionModule(`/${rel}`))
			.filter((rel) => existsSync(resolve(UI_ROOT, rel)))
			.sort();

		const actual = new Map<string, SuppressionCounts>();
		const scanErrors: string[] = [];

		for (const rel of files) {
			const source = readFileSync(resolve(UI_ROOT, rel), 'utf8');
			const { counts, errors } = scanSuppressionSource(source, rel);
			scanErrors.push(...errors);
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

		const errors: string[] = [...scanErrors];
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
					errors.push(
						`${rel}: ${key} shrank from ${max[key]} to ${c[key]}; lower suppressions.allow`
					);
				}
			}
		}

		for (const rel of allow.keys()) {
			if (!actual.has(rel)) {
				errors.push(`${rel}: zero suppressions/effects left; delete entry from suppressions.allow`);
			}
		}

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
