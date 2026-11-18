import { defineConfig } from '@playwright/test';

/**
 * A3 B6: the browser against a real `agentic-org serve --name` instance that `agentic-org setup`
 * started under a disposable root (debug build, test-support: development key folder, child
 * service manager; never the Keychain, launchd or a real organization root). Run after
 * `cargo build -p agentic-org --features test-support -p agentic-org-office -p agentic-org-web`
 * and `bun run build`. Separate from the mocked suite (`e2e/`).
 */
export default defineConfig({
	testDir: 'e2e-real',
	workers: 1,
	fullyParallel: false,
	retries: 0,
	// Debug builds hash passwords unoptimized: a sign-in answer takes seconds.
	timeout: 240_000,
	expect: { timeout: 30_000 },
	globalSetup: './e2e-real/instance.ts',
	globalTeardown: './e2e-real/teardown.ts',
	outputDir: 'test-results/real-instance',
	use: { browserName: 'chromium', viewport: { width: 1280, height: 860 }, trace: 'retain-on-failure' },
	projects: [{ name: 'real-instance', testMatch: '*.spec.ts' }]
});
