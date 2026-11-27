import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';

interface BundleBudget {
	shellJsGzipKb: number;
	shellCssGzipKb: number;
	shellTotalGzipKb: number;
	routeMaxJsGzipKb: number;
	routeOverrides: Record<string, number>;
}

const UI_ROOT = resolve(import.meta.dirname, '../../../..');
const BUDGET_PATH = resolve(UI_ROOT, 'bundle-budget.json');

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

describe('UI bundle-budget.json ratchet', () => {
	it('keeps budgets valid and strictly shrink-only relative to base', () => {
		const current = JSON.parse(readFileSync(BUDGET_PATH, 'utf8')) as BundleBudget;
		const errors: string[] = [];

		if (current.routeMaxJsGzipKb > 150) {
			errors.push(`routeMaxJsGzipKb (${current.routeMaxJsGzipKb}) must stay <= 150`);
		}
		for (const [route, kb] of Object.entries(current.routeOverrides)) {
			if (kb <= current.routeMaxJsGzipKb) {
				errors.push(`routeOverrides["${route}"] (${kb}) <= ${current.routeMaxJsGzipKb}; delete entry`);
			}
		}

		const baseRef = resolveBaseRef();
		if (baseRef) {
			try {
				const baseText = execFileSync(
					'git',
					['show', `${baseRef}:apps/agentic-org-ui/bundle-budget.json`],
					{ cwd: UI_ROOT, encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] }
				);
				const base = JSON.parse(baseText) as BundleBudget;
				for (const key of [
					'shellJsGzipKb',
					'shellCssGzipKb',
					'shellTotalGzipKb',
					'routeMaxJsGzipKb'
				] as const) {
					if (current[key] > base[key]) {
						errors.push(`${key} raised from ${base[key]} to ${current[key]} (shrink-only)`);
					}
				}
				for (const [route, kb] of Object.entries(current.routeOverrides)) {
					const prev = base.routeOverrides[route];
					if (prev === undefined) {
						errors.push(`routeOverrides added new route "${route}" (${kb} kB)`);
					} else if (kb > prev) {
						errors.push(`routeOverrides["${route}"] raised from ${prev} to ${kb}`);
					}
				}
			} catch {
				// Base predates bundle-budget.json.
			}
		}

		expect(errors).toEqual([]);
	});
});
