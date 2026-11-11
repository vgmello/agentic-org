import { afterEach, expect, it, vi } from 'vitest';
import { loadConfigFromFile } from 'vite';
import { testCspPolicy } from '../../../e2e-csp/vite-policy';

afterEach(() => vi.unstubAllEnvs());
it.each([
	['0', 'serve', 'e2e', false, false],
	['1', 'build', 'e2e', false, false],
	['1', 'serve', 'development', false, false],
	['1', 'serve', 'e2e', true, false],
	['1', 'serve', 'e2e', false, true]
] as const)('registers policy/cache only in explicit test-owned serve: flag%s %s %s preview%s', async (flag, command, mode, isPreview, enabled) => {
	vi.stubEnv('ORG_UI_CSP', flag);
	const loaded = await loadConfigFromFile({ command, mode, isPreview }, 'vite.config.ts');
	expect(loaded).not.toBeNull();
	const plugins = (loaded!.config.plugins ?? []).flat(Infinity);
	const policyIndex = plugins.findIndex((plugin) => plugin && typeof plugin === 'object' && 'name' in plugin && plugin.name === 'org-test-csp-policy');
	expect(policyIndex >= 0).toBe(enabled);
	if (enabled) expect(policyIndex).toBe(0);
	expect(loaded!.config.cacheDir).toBe(enabled ? 'node_modules/.vite-csp' : 'node_modules/.vite');
	expect(loaded!.config.server?.watch?.ignored).toEqual(['**/test-results-csp/**', '**/test-results-csp-gateway/**']);
});
it('uses a serve-only pre-plugin and registers middleware directly before Vite internals', () => {
	const plugin = testCspPolicy();
	expect(plugin.apply).toBe('serve'); expect(plugin.enforce).toBe('pre');
	const use = vi.fn();
	if (typeof plugin.configureServer !== 'function') throw new Error('No direct server hook');
	const hook = plugin.configureServer as (server: unknown) => unknown;
	expect(hook({ middlewares: { use } })).toBeUndefined();
	expect(use).toHaveBeenCalledOnce(); expect(use.mock.calls[0][0]).toBeTypeOf('function');
});
