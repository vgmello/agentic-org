import { sveltekit } from '@sveltejs/kit/vite';
import tailwindcss from '@tailwindcss/vite';
import { defineConfig } from 'vitest/config';
import { productionImportGuard } from './src/lib/platform/tooling/production-import-guard';
import { testComposition } from './e2e/vite-composition.ts';
import { testCspPolicy } from './e2e-csp/vite-policy.ts';

// Fakes are a test-owned dev-server composition. Every build (including --mode e2e) stays unavailable.
export default defineConfig(({ command, mode, isPreview }) => {
	const csp = command === 'serve' && mode === 'e2e' && !isPreview && process.env.ORG_UI_CSP === '1';
	return ({
	cacheDir: csp ? 'node_modules/.vite-csp' : 'node_modules/.vite',
	server: { watch: { ignored: ['**/test-results-csp/**', '**/test-results-csp-gateway/**'] } },
	plugins: [
		...(csp ? [testCspPolicy()] : []),
		tailwindcss(), sveltekit(),
		// Report until the remaining module moves and production-factory cutovers are complete.
		productionImportGuard({ mode: 'report' }),
		...(command === 'serve' && mode === 'e2e' && !isPreview ? [testComposition()] : [])
	],
	build: { assetsInlineLimit: 0 },
	test: { include: ['src/**/*.{test,spec}.{js,ts}'], environment: 'node' }
	});
});
