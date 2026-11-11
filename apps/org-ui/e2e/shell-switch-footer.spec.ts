import { expect, test, type Locator, type Page } from '@playwright/test';
import { visit } from './ready';
import { shot } from './shot';
import type { Connection, Organization } from '../src/lib/proto';

async function openOffice(page: Page, createPath = '/create') {
	await visit(page, createPath);
	await page.getByLabel('Organization name').fill('Footer reference');
	await page.getByRole('button', { name: 'Create organization' }).click();
	await expect(page).toHaveURL(/\/agents$/);
	await visit(page, '/offices');
}
async function switchLink(page: Page, width: number) {
	if (width < 640) {
		await page.getByRole('button', { name: 'Menu', exact: true }).press('Enter');
		return page.getByRole('dialog', { name: 'Main navigation' }).getByRole('link', { name: /^Switch organization/ });
	}
	return page.getByRole('link', { name: /^Switch organization/ });
}

for (const width of [320, 960]) {
	test(`BUG-70 footer chrome: prototype icon/text rhythm at ${width}px`, async ({ page }) => {
		await page.setViewportSize({ width, height: 900 });
		await openOffice(page);
		const link = await switchLink(page, width);
		await expect(link).toHaveAccessibleName('Switch organization (now Footer reference, HQ office on This Mac)');
		await expect(link).toHaveAttribute('href', '/welcome');
		await shot(page, `bug70-footer-${width}`, { context: link });
		expect(await link.evaluate((element) => getComputedStyle(element).columnGap)).toBe('16px');
	});

	test(`BUG-70 footer control: existing switch navigation works at ${width}px`, async ({ page }) => {
		await page.setViewportSize({ width, height: 900 });
		await openOffice(page);
		await (await switchLink(page, width)).press('Enter');
		await expect(page).toHaveURL(/\/welcome$/);
		await expect(page.getByRole('heading', { name: 'Welcome to Agentic Org', exact: true })).toBeVisible();
		await shot(page, `bug70-footer-switch-control-${width}`);
	});
}

test('BUG-70 footer control: rail keeps its existing logo switch and ten destinations', async ({ page }) => {
	await page.setViewportSize({ width: 640, height: 900 });
	await openOffice(page);
	const link = await switchLink(page, 640);
	await expect(link).toHaveAccessibleName('Switch organization (now Footer reference, HQ office on This Mac)');
	expect((await link.boundingBox())?.width).toBe(48);
	await expect(page.getByRole('navigation', { name: 'Main', exact: true }).getByRole('link')).toHaveCount(10);
	await shot(page, 'bug70-footer-rail-control');
});


const longHost = 'workstationwithaverylongname.local';
const longOffice = 'Internationalengineeringoffice';
async function longConnection(page: Page) {
	await page.evaluate(({ host, name }) => {
		// Test-owned browser storage only: seed the remembered connection and its matching mock office.
		const connections = JSON.parse(localStorage.getItem('agentic-org.mock.connections') ?? '[]') as Connection[];
		if (connections.length !== 1) throw new Error('Expected the isolated footer fixture connection');
		const current = connections[0];
		const office = { ...current.office, host, name };
		const organization = { ...current.organization, offices: current.organization.offices.map((item) => item.id === office.id ? office : item) };
		localStorage.setItem('agentic-org.mock.connections', JSON.stringify([{ office, organization }]));
		const hosted = JSON.parse(localStorage.getItem('agentic-org.mock.organizations') ?? '[]') as Organization[];
		localStorage.setItem('agentic-org.mock.organizations', JSON.stringify(hosted.map((item) => item.id === organization.id ? organization : item)));
	}, { host: longHost, name: longOffice });
	await visit(page, '/offices');
}

// Resolve modern CSS colors to sRGB and composite the actual ancestor tints (same method as look-primitives).
async function contrast(locator: Locator) {
	return locator.evaluate((element) => {
		const canvas = document.createElement('canvas');
		canvas.width = canvas.height = 1;
		const context = canvas.getContext('2d')!;
		const rgba = (color: string) => {
			context.clearRect(0, 0, 1, 1);
			context.fillStyle = color;
			context.fillRect(0, 0, 1, 1);
			return [...context.getImageData(0, 0, 1, 1).data];
		};
		const backgrounds = [];
		for (let node: Element | null = element; node; node = node.parentElement) backgrounds.push(rgba(getComputedStyle(node).backgroundColor));
		const over = (front: number[], back: number[]) => back.map((channel, i) => front[i] * front[3] / 255 + channel * (1 - front[3] / 255));
		const background = backgrounds.reverse().reduce((back, front) => over(front, back), [255, 255, 255]);
		const foreground = over(rgba(getComputedStyle(element).color), background);
		const luminance = (color: number[]) => color.map((c) => c / 255).map((c) => c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4).reduce((sum, c, i) => sum + c * [0.2126, 0.7152, 0.0722][i], 0);
		const a = luminance(foreground), b = luminance(background);
		return (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05);
	});
}
async function textFits(locator: Locator) {
	await expect(locator).not.toHaveText('');
	const result = await locator.evaluate((element) => {
		const box = element.getBoundingClientRect();
		const row = element.closest('a')!.getBoundingClientRect();
		const range = document.createRange();
		range.selectNodeContents(element);
		const rects = [...range.getClientRects()];
		return { count: rects.length, fits: rects.every((rect) => rect.left >= box.left - 1 && rect.right <= box.right + 1 && rect.top >= box.top - 1 && rect.bottom <= box.bottom + 1 && rect.left >= row.left - 1 && rect.right <= row.right + 1 && rect.top >= row.top - 1 && rect.bottom <= row.bottom + 1) };
	});
	expect(result.count).toBeGreaterThan(0);
	expect(result.fits).toBe(true);
	await expect(locator).toHaveCSS('white-space', 'normal');
	await expect(locator).toHaveCSS('overflow-x', 'visible');
}
async function footerChecks(page: Page, width: number, scale: number, host = 'This Mac', office = 'HQ') {
	const link = await switchLink(page, width);
	await link.scrollIntoViewIfNeeded();
	await expect(link).toHaveAccessibleName(`Switch organization (now Footer reference, ${office} office on ${host})`);
	await expect(link).toHaveAttribute('href', '/welcome');
	const circle = link.locator(':scope > span[aria-hidden="true"]');
	await expect(circle).toHaveCount(1);
	await expect(circle).toHaveCSS('width', `${32 * scale}px`);
	await expect(circle).toHaveCSS('height', `${32 * scale}px`);
	await expect(circle).toHaveCSS('outline-width', '1px');
	await expect(circle).toHaveCSS('outline-style', 'solid');
	expect(await circle.evaluate((element) => parseFloat(getComputedStyle(element).borderRadius))).toBeGreaterThanOrEqual(16 * scale);
	await expect(circle.locator('svg')).toHaveAttribute('aria-hidden', 'true');
	await expect(circle.locator('svg path')).toHaveCount(1);
	await expect(link).toHaveCSS('column-gap', `${16 * scale}px`);
	const primary = link.getByText(host, { exact: true });
	const secondary = link.getByText(`${office} office · Switch organization`, { exact: true });
	const textWidth = await primary.evaluate((element) => element.parentElement!.getBoundingClientRect().width);
	const availableWidth = await link.evaluate((element) => {
		const style = getComputedStyle(element);
		return element.getBoundingClientRect().width - parseFloat(style.paddingLeft) - parseFloat(style.paddingRight);
	});
	// Reserve readable text width; at text zoom the decorative circle can take its own line.
	expect(textWidth).toBeGreaterThanOrEqual(Math.min(128 * scale, availableWidth) - 1);
	await expect(primary).toHaveCSS('font-weight', '600');
	await textFits(primary);
	await textFits(secondary);
	await page.mouse.move(width - 1, 0);
	const restingContrast = await contrast(secondary);
	expect(restingContrast).toBeGreaterThanOrEqual(4.5);
	await link.hover();
	const hoverContrast = await contrast(secondary);
	expect(hoverContrast).toBeGreaterThanOrEqual(4.5);
	console.log(`Footer contrast ${width}px scale${scale}: resting=${restingContrast.toFixed(2)} hover=${hoverContrast.toFixed(2)}`);
	const bounds = (await link.boundingBox())!;
	expect(bounds.height).toBeGreaterThanOrEqual(44);
	const panel = await link.evaluate((element) => element.closest('nav')!.parentElement!.getBoundingClientRect().toJSON());
	expect(bounds.x).toBeCloseTo(panel.x, 0);
	expect(bounds.width).toBeCloseTo(panel.width, 0);
	await page.keyboard.press('Tab');
	await link.focus();
	await expect(link).toBeFocused();
	await expect(link).toHaveCSS('outline-width', '2px');
	await expect(link).toHaveCSS('outline-style', 'solid');
	return link;
}

for (const width of [320, 640, 960, 1280]) {
	for (const scale of [1, 2]) {
		for (const long of [false, true]) {
			test(`BUG-70 footer preflight: ${long ? 'long strings' : 'current data'} at ${width}px and ${scale * 100}% text`, async ({ page }) => {
				await page.setViewportSize({ width, height: 900 });
				await openOffice(page);
				if (long) await longConnection(page);
				await page.evaluate((scale) => { document.documentElement.style.fontSize = `${scale * 100}%`; }, scale);
				if (width >= 640 && width < 960) {
					const link = await switchLink(page, width);
					expect((await link.boundingBox())!.width).toBe(48 * scale);
					await expect(page.getByRole('navigation', { name: 'Main', exact: true }).getByRole('link')).toHaveCount(10);
					await shot(page, `bug70-footer-rail-${width}-text${scale * 100}-${long ? 'long' : 'current'}`);
				} else {
					const link = await footerChecks(page, width, scale, long ? longHost : undefined, long ? longOffice : undefined);
					await shot(page, `bug70-footer-${width}-text${scale * 100}-${long ? 'long' : 'current'}`, { context: link });
				}
			});
		}
	}
}

for (const width of [320, 1280]) {
	test(`BUG-70 footer hit area: padding activates the existing whole link at ${width}px`, async ({ page }) => {
		await page.setViewportSize({ width, height: 900 });
		await openOffice(page);
		const link = await switchLink(page, width);
		await link.click({ position: { x: 4, y: 4 } });
		await expect(page).toHaveURL(/\/welcome$/);
	});
}


for (const [path, label] of [
	['/agents?org=empty', 'Agents'], ['/offices?org=fails', 'Offices'], ['/chat?chat=offline&c=c-general', 'Chat']
] as const) {
	test(`BUG-70 footer fixture: ${path} preserves the current connection words`, async ({ page }) => {
		await page.setViewportSize({ width: 320, height: 900 });
		await openOffice(page);
		await visit(page, path);
		await expect(page.locator('[data-shell-header]')).toContainText(label);
		if (label === 'Agents') await expect(page.getByTestId('no-agents')).toBeVisible();
		else if (label === 'Offices') await expect(page.getByTestId('count-error')).toBeVisible();
		else await expect(page.getByTestId('office-down')).toBeVisible();
		const link = await footerChecks(page, 320, 1);
		await shot(page, `bug70-footer-fixture-${label.toLowerCase()}`, { context: link });
	});
}

test('BUG-70 footer fixture: a stopped office keeps its truthful name and switch action', async ({ page }) => {
	await page.setViewportSize({ width: 1280, height: 900 });
	await openOffice(page, '/create?office=crashed');
	await expect(page.getByTestId('office-row')).toContainText('It stopped unexpectedly');
	const link = await footerChecks(page, 1280, 1);
	await shot(page, 'bug70-footer-fixture-stopped', { context: link });
});

for (const width of [320, 1280]) {
	test(`BUG-70 footer paths: all shell destinations retain the footer at ${width}px`, async ({ page }) => {
		test.setTimeout(120_000);
		await page.setViewportSize({ width, height: 900 });
		await openOffice(page);
		for (const path of ['/chat', '/agents', '/org-chart', '/projects', '/projects/beacon', '/work', '/sessions', '/activity', '/processes', '/offices', '/settings']) {
			await visit(page, path);
			const link = await footerChecks(page, width, 1);
			await shot(page, `bug70-footer-path-${path.slice(1).replaceAll('/', '-')}-${width}`, { context: link });
			if (width < 640) {
				await page.keyboard.press('Escape');
				await expect(page.getByRole('button', { name: 'Menu', exact: true })).toBeFocused();
			}
		}
	});
}
