import { resolve } from 'node:path';
import { ESLint } from 'eslint';
import { describe, expect, it } from 'vitest';
import { findDisallowedCssUsage } from './file-size-ratchet';
import { classifyPath, findCyclicSccs, resolveTargetRel } from './module-graph';
import { scanSuppressionSource } from './suppressions-ratchet';

const UI_ROOT = resolve(import.meta.dirname, '../../../..');
const SRC_LIB = resolve(UI_ROOT, 'src/lib');

// Use real tracked file paths for lintText() so typescript-eslint's projectService resolves tsconfig.
const PLATFORM_TS_TARGET = 'src/lib/platform/shell/shell-command.ts';
const MODULE_TS_TARGET = 'src/lib/modules/home/model/brief.ts';
const MODULE_SVELTE_TARGET = 'src/lib/modules/home/features/HomeScrollHint.svelte';

const eslint = new ESLint({ cwd: UI_ROOT });

async function lintText(code: string, relPath: string): Promise<string[]> {
	const results = await eslint.lintText(code, { filePath: resolve(UI_ROOT, relPath) });
	return results.flatMap((r) =>
		r.messages.filter((m) => m.severity === 2).map((m) => m.ruleId ?? `fatal:${m.message}`)
	);
}

describe('Adversarial gate self-tests (proves ESLint + static gates fail closed)', () => {
	it('blocks domain barrel imports from platform/* AND (shell)/+layout.svelte (no flat-config overwrite)', async () => {
		const rules1 = await lintText(
			`import { something } from '$lib/modules/tess';\nexport const x = something;\n`,
			PLATFORM_TS_TARGET
		);
		expect(rules1).toContain('no-restricted-imports');

		const rules2 = await lintText(
			`import { something } from '../../modules/tess';\nexport const x = something;\n`,
			PLATFORM_TS_TARGET
		);
		expect(rules2).toContain('no-restricted-imports');

		const rules3 = await lintText(
			`<script lang="ts">import { something } from '$lib/modules/tess'; void something;</script>\n`,
			'src/routes/(shell)/+layout.svelte'
		);
		expect(rules3).toContain('no-restricted-imports');
	}, 60_000);

	it('blocks plain .css imports (including ?inline/?raw) and <style>@import</style>', async () => {
		const rules1 = await lintText(
			`import './styles.css';\nexport const x = 1;\n`,
			MODULE_TS_TARGET
		);
		expect(rules1).toContain('no-restricted-imports');

		const rules2 = await lintText(
			`import css from './styles.css?inline';\nexport const x = css;\n`,
			MODULE_TS_TARGET
		);
		expect(rules2).toContain('no-restricted-imports');

		expect(
			findDisallowedCssUsage(MODULE_SVELTE_TARGET, `<style>@import './a.css';</style>`)
		).toContain('@import');
		expect(findDisallowedCssUsage(MODULE_TS_TARGET, `import css from './a.css?raw';`)).toContain(
			'plain .css'
		);
	}, 60_000);

	it('blocks raw fetch, self.fetch, destructured fetch, EventSource, localStorage, sessionStorage, DOM sinks, and X-Org-Session-Proof', async () => {
		const cases: Array<[string, string]> = [
			[`export const r = () => fetch('/api/x');\n`, 'no-restricted-globals'],
			[`export const r = () => self.fetch('/api/x');\n`, 'no-restricted-syntax'],
			[
				`const { fetch: f } = window;\nexport const r = () => f('/api/x');\n`,
				'no-restricted-syntax'
			],
			[`export const s = () => new EventSource('/api/events');\n`, 'no-restricted-globals'],
			[`export const s = () => localStorage.getItem('k');\n`, 'no-restricted-globals'],
			[`export const s = () => sessionStorage.getItem('k');\n`, 'no-restricted-globals'],
			[`export const s = () => window.sessionStorage.getItem('k');\n`, 'no-restricted-syntax'],
			[`export function f(el: HTMLElement) { el.innerHTML = 'x'; }\n`, 'no-restricted-syntax'],
			[`export const h = 'X-Org-Session-Proof';\n`, 'no-restricted-syntax'],
			[
				`import { SESSION_PROOF_HEADER } from '$lib/platform/http/wire';\nexport const h = SESSION_PROOF_HEADER;\n`,
				'no-restricted-imports'
			],
			[`export function f() { Promise.resolve(1); }\n`, '@typescript-eslint/no-floating-promises']
		];

		for (const [code, expectedRule] of cases) {
			const rules = await lintText(code, MODULE_TS_TARGET);
			expect(rules, `Expected ${expectedRule} for: ${code}`).toContain(expectedRule);
		}
	}, 60_000);

	it('blocks missing button type, unsafe target=_blank, and {@html} in Svelte templates', async () => {
		const btnRules = await lintText(`<button>Click</button>\n`, MODULE_SVELTE_TARGET);
		expect(btnRules).toContain('svelte/button-has-type');

		const linkRules = await lintText(
			`<a href="https://example.com" target="_blank">Ex</a>\n`,
			MODULE_SVELTE_TARGET
		);
		expect(linkRules).toContain('svelte/no-target-blank');

		const htmlRules = await lintText(
			`<script lang="ts">let raw = '<b>x</b>';</script>\n{@html raw}\n`,
			MODULE_SVELTE_TARGET
		);
		expect(htmlRules).toContain('svelte/no-at-html-tags');
	}, 20_000);

	it('blocks inline ESLint config comments, file-wide eslint-disable, and unreasoned suppressions', () => {
		const inlineCfg = scanSuppressionSource(
			`/* eslint no-restricted-globals: "off" */\nexport const x = fetch('/a');\n`,
			MODULE_TS_TARGET
		);
		expect(inlineCfg.errors.some((e) => e.includes('inline ESLint configuration'))).toBe(true);

		const blockDisable = scanSuppressionSource(
			`/* eslint-disable no-restricted-globals -- bypass */\nexport const x = fetch('/a');\n`,
			MODULE_TS_TARGET
		);
		expect(blockDisable.errors.some((e) => e.includes('file-wide/block eslint-disable'))).toBe(
			true
		);

		const noReason = scanSuppressionSource(
			`// eslint-disable-next-line svelte/no-navigation-without-resolve\nexport const x = 1;\n`,
			MODULE_TS_TARGET
		);
		expect(noReason.errors.some((e) => e.includes('-- <reason>'))).toBe(true);

		const validLine = scanSuppressionSource(
			`// eslint-disable-next-line svelte/no-navigation-without-resolve -- dynamic suffix after resolve()\nexport const x = 1;\n`,
			MODULE_TS_TARGET
		);
		expect(validLine.errors).toEqual([]);
		expect(validLine.counts.eslint).toBe(1);
	});

	it('classifies bare module barrels, extensionless platform files, and src/lib root modules and detects cycles', () => {
		expect(classifyPath('src/lib/modules/members')).toEqual({
			kind: 'module',
			domain: 'members',
			unit: 'module:members'
		});
		expect(classifyPath('src/lib/platform/unavailable-api')).toEqual({
			kind: 'platform',
			unit: 'platform:unavailable-api'
		});
		expect(classifyPath('src/lib/nav.ts')).toEqual({
			kind: 'lib-root',
			unit: 'lib-root:nav'
		});
		expect(classifyPath('src/lib/nav')).toEqual({
			kind: 'lib-root',
			unit: 'lib-root:nav'
		});
		expect(resolveTargetRel('$lib/modules/members', resolve(SRC_LIB, 'nav.ts'), SRC_LIB)).toBe(
			'src/lib/modules/members'
		);

		const cyclic = new Map<string, Set<string>>([
			['components', new Set(['lib-root:nav'])],
			['lib-root:nav', new Set(['module:organization'])],
			['module:organization', new Set(['components'])]
		]);
		expect(findCyclicSccs(cyclic)).toEqual([['components', 'lib-root:nav', 'module:organization']]);
	});
});
