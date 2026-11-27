import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';

interface BundleBudget {
	shellJsGzipKb: number;
	shellCssGzipKb: number;
	shellTotalGzipKb: number;
	onboardingShellJsGzipKb: number;
	onboardingShellCssGzipKb: number;
	onboardingShellTotalGzipKb: number;
	nestedLayoutMaxJsGzipKb: number;
	nestedLayoutOverrides: Record<string, number>;
	routeMaxJsGzipKb: number;
	routeOverrides: Record<string, number>;
	onboardingRouteMaxJsGzipKb: number;
	onboardingRouteOverrides: Record<string, number>;
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

function checkOverrides(
	label: string,
	overrides: Record<string, number>,
	defaultMax: number,
	baseOverrides: Record<string, number> | undefined,
	errors: string[]
): void {
	for (const [route, kb] of Object.entries(overrides)) {
		if (kb <= defaultMax) {
			errors.push(`${label}["${route}"] (${kb}) <= ${defaultMax}; delete entry`);
		}
		if (!baseOverrides) continue;
		const prev = baseOverrides[route];
		if (prev === undefined) {
			errors.push(`${label} added new route "${route}" (${kb} kB)`);
		} else if (kb > prev) {
			errors.push(`${label}["${route}"] raised from ${prev} to ${kb}`);
		}
	}
}

describe('UI bundle-budget.json ratchet', () => {
	it('keeps budgets valid and strictly shrink-only relative to base', () => {
		const current = JSON.parse(readFileSync(BUDGET_PATH, 'utf8')) as BundleBudget;
		const errors: string[] = [];

		for (const [k, v] of [
			['routeMaxJsGzipKb', current.routeMaxJsGzipKb],
			['nestedLayoutMaxJsGzipKb', current.nestedLayoutMaxJsGzipKb],
			['onboardingRouteMaxJsGzipKb', current.onboardingRouteMaxJsGzipKb]
		] as const) {
			if (v > 150) errors.push(`${k} (${v}) must stay <= 150`);
		}

		let base: BundleBudget | undefined;
		const baseRef = resolveBaseRef();
		if (baseRef) {
			try {
				const baseText = execFileSync(
					'git',
					['show', `${baseRef}:apps/agentic-org-ui/bundle-budget.json`],
					{ cwd: UI_ROOT, encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] }
				);
				base = JSON.parse(baseText) as BundleBudget;
			} catch {
				// Base predates bundle-budget.json.
			}
		}

		if (base) {
			for (const key of [
				'shellJsGzipKb',
				'shellCssGzipKb',
				'shellTotalGzipKb',
				'onboardingShellJsGzipKb',
				'onboardingShellCssGzipKb',
				'onboardingShellTotalGzipKb',
				'nestedLayoutMaxJsGzipKb',
				'routeMaxJsGzipKb',
				'onboardingRouteMaxJsGzipKb'
			] as const) {
				if (typeof base[key] === 'number' && current[key] > base[key]) {
					errors.push(`${key} raised from ${base[key]} to ${current[key]} (shrink-only)`);
				}
			}
		}

		checkOverrides(
			'nestedLayoutOverrides',
			current.nestedLayoutOverrides,
			current.nestedLayoutMaxJsGzipKb,
			base?.nestedLayoutOverrides,
			errors
		);
		checkOverrides(
			'routeOverrides',
			current.routeOverrides,
			current.routeMaxJsGzipKb,
			base?.routeOverrides,
			errors
		);
		checkOverrides(
			'onboardingRouteOverrides',
			current.onboardingRouteOverrides,
			current.onboardingRouteMaxJsGzipKb,
			base?.onboardingRouteOverrides,
			errors
		);

		expect(errors).toEqual([]);
	});
});
