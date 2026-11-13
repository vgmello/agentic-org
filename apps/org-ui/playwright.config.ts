import { defineConfig } from '@playwright/test';

const port = Number(process.env.PW_PORT) || 4173;
const productionPort = port + 1;
// Run native WebKit pointer regressions with PW_WEBKIT=1 and --project=webkit;
// the webkit project selects the cases tagged '@webkit' on their test( line.
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
		{ name: 'chromium-default-font', testMatch: 'non-chat-icons-default-font.ts', use: { browserName: 'chromium', baseURL: `http://localhost:${port}`, launchOptions: { args: ['--blink-settings=defaultFontSize=20'] } } },
		{ name: 'chromium-chat-default-font', testMatch: 'chat-followups-default-font.ts', use: { browserName: 'chromium', baseURL: `http://localhost:${port}`, launchOptions: { args: ['--blink-settings=defaultFontSize=20'] } } },
		{ name: 'chromium-focus-default-font', testMatch: 'focus-primitives-default-font.ts', use: { browserName: 'chromium', baseURL: `http://localhost:${port}`, launchOptions: { args: ['--blink-settings=defaultFontSize=20'] } } },
		{ name: 'chromium-title-default-font', testMatch: 'dialog-title-words-default-font.ts', use: { browserName: 'chromium', baseURL: `http://localhost:${port}`, launchOptions: { args: ['--blink-settings=defaultFontSize=20'] } } },
		...(webkit ? [{
			name: 'webkit',
			testIgnore: 'production-unavailable.spec.ts',
			grep: /@webkit/,
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
