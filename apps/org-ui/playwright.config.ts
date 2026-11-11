import { defineConfig } from '@playwright/test';

const port = Number(process.env.PW_PORT) || 4173;
const productionPort = port + 1;
// Run native WebKit pointer regressions with PW_WEBKIT=1 and --project=webkit.
// The default suite continues to work with only Chromium installed.
const webkit = process.env.PW_WEBKIT === '1';

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
		{ name: 'production', testMatch: 'production-unavailable.spec.ts', use: { browserName: 'chromium', baseURL: `http://localhost:${productionPort}` } },
		...(webkit ? [{
			name: 'webkit',
			testMatch: ['offices.spec.ts', 'shell-responsive.spec.ts', 'chat-lows.spec.ts', 'chat-thread-focus.spec.ts', 'chat-live-delivery.spec.ts', 'chat-layout.spec.ts', 'look-primitives.spec.ts'],
			grep: /BUG-27|BUG-24: pointer|BUG-59|BUG-25|BUG-26|BUG-64|BUG-62|BUG-65|BUG-66|BUG-67|BUG-68|BUG-71|BUG-70/,
			use: { browserName: 'webkit' as const, baseURL: `http://localhost:${port}` }
		}] : [])
	],
	webServer: [
		// bun run test:e2e builds before either server starts.
		// Only this explicit test-owned server composes fakes; ordinary dev remains unavailable.
		{ command: `bun run dev --mode e2e --port ${port} --strictPort`, url: `http://localhost:${port}`, reuseExistingServer: false },
		// Exercise the actual static release, including direct paths and query/global bypass attempts.
		{ command: `bun run preview --host 127.0.0.1 --port ${productionPort} --strictPort`, url: `http://localhost:${productionPort}`, reuseExistingServer: false }
	]
});
