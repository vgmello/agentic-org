import { expect, test } from '@playwright/test';
import { randomBytes } from 'node:crypto';
import { origin, snapshot } from './collector';

test('B1-05 Part B: this browser reports a blocked inline control before its suite', async ({
	page,
	browserName
}) => {
	const marker = `c${randomBytes(4).toString('hex')}`;
	// A signed-out visitor stays on sign-in with its query; /welcome would send them there and drop it (SIGN-7).
	await page.goto(`/sign-in?csp-control=${marker}`);
	await expect(page.locator('html')).toHaveAttribute('data-hydrated', 'true');
	await expect(page.getByRole('heading', { level: 1, name: /^Sign in to HQ at / })).toBeVisible();
	expect(new URL(page.url()).searchParams.get('csp-control')).toBe(marker);
	await page.evaluate((value) => {
		const script = document.createElement('script');
		script.textContent = `/*${value}*/window.cspControlRan=true`;
		document.head.append(script);
	}, marker);
	await expect
		.poll(
			async () =>
				(await snapshot()).reports.filter(
					(report) =>
						new URL(report.documentURI).searchParams.get('csp-control') === marker &&
						report.blockedURI === 'inline' &&
						['script-src', 'script-src-elem'].includes(report.directive) &&
						(browserName === 'webkit' || report.sample.includes(marker))
				).length
		)
		.toBeGreaterThan(0);
	expect(
		await page.evaluate(() => (window as Window & { cspControlRan?: boolean }).cspControlRan)
	).toBeUndefined();
	const clear = await fetch(`${origin}/__csp-clear`, {
		method: 'POST',
		body: JSON.stringify({ marker, browser: browserName })
	});
	expect(clear.status).toBe(200);
});
