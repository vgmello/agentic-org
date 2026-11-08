import { defineConfig } from '@playwright/test';

// Several people (and agents) run this suite on one machine: PW_PORT picks another port when 4173 is taken.
const port = Number(process.env.PW_PORT ?? 4173);

// UI flows run in a browser against `bun run dev`, with the TypeScript office mock
// (src/lib/office/mock.ts). The Rust shell is covered by `cargo test`.
export default defineConfig({
	testDir: 'e2e',
	fullyParallel: true,
	// Generous timeouts: the suite also runs on busy machines (many builds at once), and the mock
	// office takes ~1 s to "start" on purpose.
	timeout: 60_000,
	expect: { timeout: 15_000 },
	forbidOnly: !!process.env.CI,
	// Everything Playwright writes (including a screenshot after every test) goes to test-results/,
	// which is in .gitignore.
	outputDir: 'test-results',
	use: { baseURL: `http://localhost:${port}`, viewport: { width: 1440, height: 900 }, screenshot: 'on' },
	projects: [{ name: 'chromium', use: { browserName: 'chromium' } }],
	// Its own port, so it never reuses (or collides with) the dev server that `tauri dev` runs on 1420.
	webServer: { command: `bun run dev --port ${port}`, url: `http://localhost:${port}`, reuseExistingServer: false }
});
