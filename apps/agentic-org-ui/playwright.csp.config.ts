import { defineConfig } from '@playwright/test';
import base from './playwright.config';

const port = Number(process.env.CSP_PW_PORT ?? '4373');
if (!Number.isInteger(port) || port < 1 || port > 65535) throw new Error('Invalid CSP_PW_PORT');
const browsers = ['chromium', 'webkit', 'firefox'] as const;
export default defineConfig({
	...base, testDir: 'e2e-csp', outputDir: 'test-results-csp', globalTeardown: './e2e-csp/teardown.ts',
	projects: browsers.flatMap((browserName) => [
		{ name: `csp-control-${browserName}`, testDir: 'e2e-csp', testMatch: 'csp-control.spec.ts', use: { browserName, baseURL: `http://localhost:${port}` } },
		{ name: `csp-${browserName}`, testDir: 'e2e-csp', testIgnore: 'csp-control.spec.ts', testMatch: browserName === 'chromium' ? '**/*.spec.ts' : 'csp-smoke.spec.ts', dependencies: [`csp-control-${browserName}`], use: { browserName, baseURL: `http://localhost:${port}` } },
		...(browserName === 'chromium' ? [{ name: 'csp-chromium-full', testDir: 'e2e', testIgnore: ['shot.spec.ts', 'production-unavailable.spec.ts'], dependencies: ['csp-control-chromium'], use: { browserName, baseURL: `http://localhost:${port}` } }] : [])
	]),
	webServer: { command: `ORG_UI_CSP=1 bun run dev --mode e2e --port ${port} --strictPort`, url: `http://localhost:${port}/__csp-status`, reuseExistingServer: false, timeout: 60_000 }
});
