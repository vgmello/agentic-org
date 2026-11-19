import { afterEach, expect, it, vi } from 'vitest';

afterEach(() => { vi.unstubAllEnvs(); vi.resetModules(); });
it.each(['', '0', '65536', '1.5', 'not-a-port'])('refuses invalid CSP_PW_PORT %s before starting a server', async (value) => {
	vi.stubEnv('CSP_PW_PORT', value);
	vi.resetModules();
	await expect(import('../../../playwright.csp.config')).rejects.toThrow('Invalid CSP_PW_PORT');
});
it.each([
	[undefined, 4373], ['4593', 4593], ['4.593e3', 4593], ['65535', 65535]
] as const)('uses the same normalized CSP origin for server, projects and collector: %s', async (value, port) => {
	vi.stubEnv('CSP_PW_PORT', value);
	vi.resetModules();
	const { default: config } = await import('../../../playwright.csp.config');
	const collector = await import('../../../e2e-csp/collector');
	const origin = `http://localhost:${port}`;
	expect(collector.origin).toBe(origin);
	expect(config.projects).toHaveLength(7);
	for (const project of config.projects!) expect(project.use?.baseURL).toBe(origin);
	const server = Array.isArray(config.webServer) ? config.webServer[0] : config.webServer;
	expect(server?.url).toBe(`${origin}/__csp-status`);
	expect(server?.command).toContain(`--port ${port} --strictPort`);
});
it('wires every browser control before its suite, isolates collection and retains the approved limits', async () => {
	vi.resetModules();
	const { default: config } = await import('../../../playwright.csp.config');
	expect(config.timeout).toBe(60_000);
	expect(config.expect?.timeout).toBe(15_000);
	expect(config.use?.viewport).toEqual({ width: 1440, height: 900 });
	expect(config.use?.screenshot).toBe('on');
	expect(config.globalTeardown).toBe('./e2e-csp/teardown.ts');
	expect(config.outputDir).toBe('test-results-csp');
	expect(config.projects).toHaveLength(7);
	for (const browser of ['chromium', 'webkit', 'firefox']) {
		const control = config.projects!.find((p) => p.name === `csp-control-${browser}`);
		expect(control?.testDir).toBe('e2e-csp');
		expect(control?.testMatch).toBe('csp-control.spec.ts');
		const smoke = config.projects!.find((p) => p.name === `csp-${browser}`);
		expect(smoke?.testDir).toBe('e2e-csp');
		expect(smoke?.testIgnore).toBe('csp-control.spec.ts');
		expect(smoke?.testMatch).toBe(browser === 'chromium' ? '**/*.spec.ts' : 'csp-smoke.spec.ts');
		expect(smoke?.dependencies).toEqual([`csp-control-${browser}`]);
	}
	const full = config.projects!.find((p) => p.name === 'csp-chromium-full');
	expect(full?.testDir).toBe('e2e');
	expect(full?.testIgnore).toEqual(['shot.spec.ts', 'production-unavailable.spec.ts']);
	expect(full?.dependencies).toEqual(['csp-control-chromium']);
	const server = Array.isArray(config.webServer) ? config.webServer[0] : config.webServer;
	expect(server?.command).toMatch(/^ORG_UI_CSP=1 bun run dev --mode e2e /);
	expect(server?.reuseExistingServer).toBe(false);
});
