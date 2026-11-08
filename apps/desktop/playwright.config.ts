import { defineConfig } from '@playwright/test';

// UI flows run in a browser against `bun run dev`, with the TypeScript office mock
// (src/lib/office/mock.ts). The Rust shell is covered by `cargo test`.
export default defineConfig({
	testDir: 'e2e',
	fullyParallel: true,
	forbidOnly: !!process.env.CI,
	// Everything Playwright writes (including a screenshot after every test) goes to test-results/,
	// which is in .gitignore.
	outputDir: 'test-results',
	use: { baseURL: 'http://localhost:4173', viewport: { width: 1440, height: 900 }, screenshot: 'on' },
	projects: [{ name: 'chromium', use: { browserName: 'chromium' } }],
	// Its own port, so it never reuses (or collides with) the dev server that `tauri dev` runs on 1420.
	webServer: { command: 'bun run dev --port 4173', url: 'http://localhost:4173', reuseExistingServer: false }
});
