import { defineConfig } from '@playwright/test';

// The pinned runner otherwise writes an automatic full-page error snapshot that
// can contain the generated login. Test errors are also redacted in afterEach.
process.env.PLAYWRIGHT_NO_COPY_PROMPT = '1';

// No Vite/Node backend. The owning protected setup/adoption driver must supply
// SELECTED_TASK_FIXTURE and keep the real supervised Rust services alive.
export default defineConfig({
	testDir: 'e2e-selected',
	outputDir: 'test-results-selected',
	workers: 1,
	fullyParallel: false,
	forbidOnly: true,
	retries: 0,
	timeout: 600_000,
	expect: { timeout: 15_000 },
	reporter: [['list']],
	use: {
		actionTimeout: 30_000,
		navigationTimeout: 30_000,
		trace: 'off',
		video: 'off',
		// Explicit screenshots mask generated logins and inputs. Automatic failure
		// screenshots cannot guarantee that redaction, including during sign-in.
		screenshot: 'off',
		viewport: { width: 1280, height: 900 },
		reducedMotion: 'reduce'
	},
	// Run one explicitly selected project per fresh protected fixture. The final
	// registry revocation invalidates that fixture owner account permanently.
	projects: [
		{ name: 'selected-chromium', use: { browserName: 'chromium' } },
		{ name: 'selected-webkit', use: { browserName: 'webkit' } }
	]
});
