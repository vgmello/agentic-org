import { sveltekit } from '@sveltejs/kit/vite';
import tailwindcss from '@tailwindcss/vite';
import type { ProxyOptions } from 'vite';
import { defineConfig } from 'vitest/config';
import { productionImportGuard } from './src/lib/platform/tooling/production-import-guard.ts';
import { testComposition } from './e2e/vite-composition.ts';
import { testCspPolicy } from './e2e-csp/vite-policy.ts';

function buildLiveApiProxy(
	rawTarget: string | undefined
): Record<string, ProxyOptions> | undefined {
	const target = rawTarget?.trim();
	if (!target) return undefined;
	const targetOrigin = new URL(target).origin;
	return {
		'/api': {
			target,
			changeOrigin: true,
			configure(proxy) {
				proxy.on('proxyReq', (proxyReq, req) => {
					// agentic-org-web RequestAdmission enforces exact Origin matching on POST and whenever Origin is sent.
					if (req.headers.origin || req.method === 'POST') {
						proxyReq.setHeader('origin', targetOrigin);
					}
					const referer = typeof req.headers.referer === 'string' ? req.headers.referer : undefined;
					if (referer) {
						try {
							const parsed = new URL(referer);
							proxyReq.setHeader('referer', `${targetOrigin}${parsed.pathname}${parsed.search}`);
						} catch {
							proxyReq.removeHeader('referer');
						}
					}
				});
			}
		}
	};
}

// Fakes are a test-owned dev-server composition. Every build (including --mode e2e) stays unavailable.
export default defineConfig(({ command, mode, isPreview }) => {
	const csp = command === 'serve' && mode === 'e2e' && !isPreview && process.env.ORG_UI_CSP === '1';
	const liveProxy =
		command === 'serve' && !isPreview && mode !== 'e2e'
			? buildLiveApiProxy(process.env.ORG_API_TARGET)
			: undefined;
	return {
		cacheDir: csp
			? 'node_modules/.vite-csp'
			: liveProxy
				? 'node_modules/.vite-live'
				: 'node_modules/.vite',
		server: {
			...(liveProxy ? { proxy: liveProxy } : {}),
			watch: { ignored: ['**/test-results-csp/**', '**/test-results-csp-gateway/**'] }
		},
		plugins: [
			...(csp ? [testCspPolicy()] : []),
			tailwindcss(),
			sveltekit(),
			// Fail the production build if any mock, sample, test fixture, or in-memory engine leaks in.
			productionImportGuard({ mode: 'error' }),
			...(command === 'serve' && mode === 'e2e' && !isPreview ? [testComposition()] : [])
		],
		build: { assetsInlineLimit: 0 },
		test: { include: ['src/**/*.{test,spec}.{js,ts}'], environment: 'node' }
	};
});
