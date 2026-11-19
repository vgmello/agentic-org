import { defineConfig } from '@playwright/test';
import base from './playwright.config';
import { gatewayPort } from './scripts/csp-gateway';

const port = gatewayPort(process.env.CSP_GW_PORT);
const origin = `http://127.0.0.1:${port}`;
export default defineConfig({
	...base,
	testDir: 'e2e-csp-gateway',
	outputDir: 'test-results-csp-gateway',
	projects: (['chromium', 'webkit', 'firefox'] as const).map((name) => ({ name, use: { ...base.use, browserName: name, baseURL: origin } })),
	webServer: { command: 'bun scripts/csp-gateway.ts serve', url: `${origin}/_app/version.json`, reuseExistingServer: false, timeout: 30_000 }
});
