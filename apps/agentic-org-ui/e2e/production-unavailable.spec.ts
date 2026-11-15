import { expect, test } from '@playwright/test';
import { visit } from './ready';
import { shot } from './shot';

const areas = [
	['agents', 'Agents'], ['org-chart', 'Org chart'], ['projects', 'Projects'], ['work', 'Work'], ['chat', 'Chat'],
	['processes', 'Processes'], ['offices', 'Offices'], ['sessions', 'Sessions'], ['activity', 'Activity'], ['settings', 'Settings']
] as const;
const heading = (area?: string) => area ? `${area} isn't available yet` : "Agentic Org isn't ready in the browser yet";
const body = (area?: string) => area
	? `This release of the server doesn't serve ${area} in the browser. Nothing is wrong with your organization. To see what this server can do now, run agentic-org doctor on the HQ machine.`
	: "This server is running, but this release doesn't serve its pages in the browser yet. To see what it can do now, run agentic-org doctor on the HQ machine.";

test('SRV-18 / BUG-53: unavailable pages explain this release and the next step without sample data or native controls', { tag: '@journey' }, async ({ page }) => {
	const apiRequests: string[] = [];
	page.on('request', (request) => { if (new URL(request.url()).pathname.startsWith('/api/')) apiRequests.push(request.url()); });
	for (const [path, area] of [
		['/', undefined], ['/welcome', undefined], ['/create', undefined],
		...areas.map(([path, name]) => [`/${path}`, name] as const), ['/projects/a-project', 'Projects']
	] as const) {
		await visit(page, path);
		await expect(page.getByTestId('area-unavailable')).toBeVisible();
		await expect(page.getByRole('main')).toHaveCount(1);
		await expect(page.locator('main main')).toHaveCount(0);
		await expect(page.getByRole('heading', { level: 1, name: heading(area), exact: true })).toBeVisible();
		await expect(page.locator('main p')).toHaveText(body(area));
		await expect(page.locator('main code')).toHaveText('agentic-org doctor');
		await expect(page).toHaveTitle(area ? `${area} · Agentic Org` : 'Agentic Org');
		await expect(page.getByRole('status')).toHaveCount(0);
		await expect(page.getByText('Sample data', { exact: true })).toHaveCount(0);
		await expect(page.locator('[data-tauri-drag-region]')).toHaveCount(0);
		await expect(page.getByRole('button')).toHaveCount(0);
	}
	expect(apiRequests).toEqual([]);
	await shot(page, 'SRV-18-unavailable');
});

test('SRV-18: browser query parameters, remembered sample data and native globals cannot enable fakes', { tag: '@journey' }, async ({ page }) => {
	await page.addInitScript(() => {
		Object.assign(window, { __TAURI_INTERNALS__: {}, __mockOfficeStatus: 'injected' });
		localStorage.setItem('agentic-org.mock.connections', JSON.stringify([{ organization: { id: 'forged', name: 'Forged sample' } }]));
	});
	await visit(page, '/org-chart?org=empty&mock=healthy&copilot=on');
	await expect(page.getByRole('heading', { name: "Org chart isn't available yet" })).toBeVisible();
	await expect(page.getByText('Forged sample')).toHaveCount(0);
	await expect(page.getByRole('button', { name: /add|create|bring in/i })).toHaveCount(0);
	await page.getByRole('link', { name: 'Chat', exact: true }).click();
	await expect(page.getByRole('heading', { name: "Chat isn't available yet" })).toBeVisible();
	await expect(page.getByRole('link', { name: 'Chat', exact: true })).toHaveAttribute('aria-current', 'page');
	await shot(page, 'SRV-18-unavailable-navigation');
});


test('SRV-18 / BUG-53: unknown routes keep their error and a named browser tab', async ({ page }) => {
	await visit(page, '/no-such-area');
	await expect(page).toHaveTitle('Page not found · Agentic Org');
	await expect(page.getByRole('heading', { name: 'Something went wrong' })).toBeVisible();
	await expect(page.getByText('404', { exact: true })).toBeVisible();
	await expect(page.getByRole('main')).toHaveCount(1);
	await expect(page.locator('main main')).toHaveCount(0);
	await expect(page.getByTestId('area-unavailable')).toHaveCount(0);
	await page.getByRole('link', { name: 'Back to the start' }).click();
	await expect(page.getByTestId('area-unavailable')).toBeVisible();
});

for (const [path, area] of [['/', undefined], ['/agents', 'Agents']] as const) {
	test(`BUG-53: ${path} keeps its wrapping navigation and readable copy at 320px`, async ({ page }) => {
		await page.setViewportSize({ width: 320, height: 900 });
		await visit(page, path);
		await expect(page.getByRole('heading', { level: 1, name: heading(area), exact: true })).toBeVisible();
		await expect(page.locator('main p')).toHaveText(body(area));
		await expect(page.getByRole('navigation', { name: 'Main', exact: true })).toBeVisible();
		await expect(page.getByRole('button')).toHaveCount(0);
		expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(320);
		expect(await page.locator('main').evaluate((el) => {
			const main = el.getBoundingClientRect();
			return [...el.querySelectorAll('h1, p, code')].every((text) => {
				const box = text.getBoundingClientRect();
				return box.left >= main.left && box.right <= main.right && text.scrollWidth <= text.clientWidth;
			});
		})).toBe(true);
		await shot(page, `unavailable-${area ?? 'root'}-320`);
	});
}

test('BUG-53: the unavailable header home link has the design-system keyboard focus ring', async ({ page }) => {
	await visit(page, '/');
	await page.keyboard.press('Tab');
	const home = page.getByRole('link', { name: 'Agentic Org', exact: true });
	await expect(home).toBeFocused();
	await shot(page, 'unavailable-home-keyboard-focus');
	const style = await home.evaluate((el) => {
		const reference = document.createElement('span');
		reference.style.cssText = 'display:none;color:var(--ring)';
		document.body.append(reference);
		const ring = getComputedStyle(reference).color;
		reference.remove();
		const css = getComputedStyle(el);
		return { ring, color: css.outlineColor, style: css.outlineStyle, width: css.outlineWidth, offset: css.outlineOffset };
	});
	expect(style.style).toBe('solid');
	expect(style.width).toBe('2px');
	expect(style.offset).toBe('2px');
	expect(style.color).toBe(style.ring);
});
