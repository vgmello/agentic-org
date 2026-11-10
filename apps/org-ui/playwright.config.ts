import { defineConfig } from '@playwright/test';

const port = Number(process.env.PW_PORT) || 4173;
const productionPort = port + 1;

export default defineConfig({
	testDir: 'e2e',
	fullyParallel: true,
	timeout: 60_000,
	expect: { timeout: 15_000 },
	forbidOnly: !!process.env.CI,
	outputDir: 'test-results',
	use: { viewport: { width: 1440, height: 900 }, screenshot: 'on' },
	projects: [
		{ name: 'chromium', testIgnore: 'production-unavailable.spec.ts', use: { browserName: 'chromium', baseURL: `http://localhost:${port}` } },
		{ name: 'production', testMatch: 'production-unavailable.spec.ts', use: { browserName: 'chromium', baseURL: `http://localhost:${productionPort}` } }
	],
	webServer: [
		// bun run test:e2e builds before either server starts.
		// Only this explicit test-owned server composes fakes; ordinary dev remains unavailable.
		{ command: `bun run dev --mode e2e --port ${port} --strictPort`, url: `http://localhost:${port}`, reuseExistingServer: false },
		// Exercise the actual static release, including direct paths and query/global bypass attempts.
		{ command: `bun run preview --host 127.0.0.1 --port ${productionPort} --strictPort`, url: `http://localhost:${productionPort}`, reuseExistingServer: false }
	]
});
