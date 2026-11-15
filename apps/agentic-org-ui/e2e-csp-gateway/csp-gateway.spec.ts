import { expect, test, type Page } from '@playwright/test';
import { randomBytes } from 'node:crypto';
import { isControl, type Violation } from './control';

const htmlPolicy = "default-src 'none'; script-src 'self' {script_hashes}; style-src 'self' 'unsafe-inline'; img-src 'self'; font-src 'self'; connect-src 'self'; manifest-src 'self'; worker-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'; object-src 'none'";
const routes = ['/', '/welcome', '/create', '/agents', '/org-chart', '/projects', '/projects/a-project', '/work', '/chat', '/processes', '/offices', '/sessions', '/activity', '/settings'];
type Capture = (message: Violation | { kind: 'barrier' }) => Promise<void>;
type CaptureWindow = Window & { captureCsp: Capture; unexpectedInlineRan?: boolean };

// One context spans fresh documents and another page, so capture cannot reset at each navigation.
test('B1-05 Part A: the real gateway CSP permits every built route and captures blocked scripts', async ({ page, context }, info) => {
	const violations: Violation[] = [];
	await context.exposeBinding('captureCsp', (_source, message: Violation | { kind: 'barrier' }) => {
		if (message.kind === 'violation') violations.push(message);
	});
	await context.addInitScript(() => {
		window.addEventListener('securitypolicyviolation', (event) => {
			void (window as CaptureWindow).captureCsp({ kind: 'violation', documentURI: event.documentURI, blockedURI: event.blockedURI, effectiveDirective: event.effectiveDirective });
		});
	});
	const barrier = (target: Page) => target.evaluate(() => (window as CaptureWindow).captureCsp({ kind: 'barrier' }));
	async function loaded(target: Page, path: string, reload = false) {
		await barrier(target);
		const response = reload ? await target.reload() : await target.goto(path);
		expect(response, `document response for ${path}`).not.toBeNull();
		expect(response!.status()).toBe(200);
		const policy = response!.headers()['content-security-policy'] ?? '';
		const hashes = policy.match(/'sha256-[A-Za-z0-9+/]{43}='/g) ?? [];
		expect(hashes.length, `bootstrap hash for ${path}`).toBeGreaterThan(0);
		expect(policy.replace(hashes.join(' '), '{script_hashes}'), `exact CSP for ${path}`).toBe(htmlPolicy);
		await expect(target.locator('html')).toHaveAttribute('data-hydrated', 'true');
		await expect(target.getByTestId('area-unavailable')).toBeVisible();
	}
	async function positiveControl(target: Page) {
		const marker = `c${randomBytes(4).toString('hex')}`;
		await loaded(target, `/welcome?csp-control=${marker}`);
		expect(new URL(target.url()).searchParams.get('csp-control')).toBe(marker);
		await barrier(target);
		expect(violations).toEqual([]);
		await target.evaluate((value) => {
			const script = document.createElement('script');
			script.textContent = `/*${value}*/window.unexpectedInlineRan=true;`;
			document.head.append(script);
		}, marker);
		const matching = (event: Violation) => isControl(event, marker);
		await expect.poll(() => violations.filter(matching).length).toBeGreaterThan(0);
		expect(await target.evaluate(() => (window as CaptureWindow).unexpectedInlineRan === true)).toBe(false);
		for (let index = violations.length - 1; index >= 0; index--) if (matching(violations[index])) violations.splice(index, 1);
	}

	await positiveControl(page);
	await loaded(page, '/welcome');
	await loaded(page, '/welcome', true);
	await page.screenshot({ path: info.outputPath('gateway-csp-reloaded-welcome.png'), fullPage: true });
	const second = await context.newPage();
	await positiveControl(second);
	await loaded(second, '/welcome');
	await barrier(second);
	await second.close();
	const visited = new Set<string>();
	for (const path of routes) await test.step(`fresh route ${path}`, async () => {
		await loaded(page, path);
		visited.add(path);
	});
	await barrier(page);
	expect(visited.size).toBe(14);
	expect(violations).toEqual([]);
	await page.screenshot({ path: info.outputPath('gateway-csp-final-route.png'), fullPage: true });
});
