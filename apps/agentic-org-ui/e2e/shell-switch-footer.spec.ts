import { expect, test, type Locator, type Page } from '@playwright/test';
import { visit } from './ready';
import { shot } from './shot';
import { signInAsAdmin } from './web-session';
import type { Connection, Organization } from '../src/lib/proto';
import { enterMockedOrganization } from './mock-organization';

// The shell footer (app-shell-ux §0): the "You · Owner" account menu (Your preferences, Switch organization, About,
// Sign out) and the office line ("HQ office · Online") that opens Organization settings › Offices. The administrator
// is signed in (SIGN-9) and GET /api/v1/organizations lists the organization opened here, so Switch organization
// lands on the signed-in organizations page. Below 640 px the footer is in the Menu drawer; from 640 to 959 px the
// rail keeps the account menu as a 48 px button and leaves the office line out.
async function openShell(page: Page, createPath = '/create') {
	await signInAsAdmin(page, [
		{
			id: 'org-footer-reference',
			name: 'Footer reference',
			created_at: '2026-11-18T00:00:00Z',
			recovery: 'acknowledged'
		}
	]);
	await enterMockedOrganization(page, 'Footer reference', createPath);
	await expect(page).toHaveURL(/\/teams\/members$/);
}
/** Where the footer is at this width: the Menu drawer below 640 px, the page otherwise. */
async function footerScope(page: Page, width: number): Promise<Locator> {
	if (width < 640) {
		await page.getByRole('button', { name: 'Menu', exact: true }).press('Enter');
		return page.getByRole('dialog', { name: 'Main navigation' });
	}
	return page.locator('body');
}
const accountButton = (scope: Locator) => scope.getByRole('button', { name: /^You · Owner/ });
const officeLine = (scope: Locator, office = 'HQ') =>
	scope.getByRole('link', { name: new RegExp(`^${office} office · `) });

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
		for (let node: Element | null = element; node; node = node.parentElement)
			backgrounds.push(rgba(getComputedStyle(node).backgroundColor));
		const over = (front: number[], back: number[]) =>
			back.map((channel, i) => (front[i] * front[3]) / 255 + channel * (1 - front[3] / 255));
		const background = backgrounds
			.reverse()
			.reduce((back, front) => over(front, back), [255, 255, 255]);
		const foreground = over(rgba(getComputedStyle(element).color), background);
		const luminance = (color: number[]) =>
			color
				.map((c) => c / 255)
				.map((c) => (c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4))
				.reduce((sum, c, i) => sum + c * [0.2126, 0.7152, 0.0722][i], 0);
		const a = luminance(foreground),
			b = luminance(background);
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
		return {
			count: rects.length,
			fits: rects.every(
				(rect) =>
					rect.left >= box.left - 1 &&
					rect.right <= box.right + 1 &&
					rect.top >= box.top - 1 &&
					rect.bottom <= box.bottom + 1 &&
					rect.left >= row.left - 1 &&
					rect.right <= row.right + 1 &&
					rect.top >= row.top - 1 &&
					rect.bottom <= row.bottom + 1
			)
		};
	});
	expect(result.count).toBeGreaterThan(0);
	expect(result.fits).toBe(true);
	await expect(locator).toHaveCSS('white-space', 'normal');
	await expect(locator).toHaveCSS('overflow-x', 'visible');
}
async function footerChecks(page: Page, width: number, scale: number, office = 'HQ') {
	const scope = await footerScope(page, width);
	const account = accountButton(scope);
	await account.scrollIntoViewIfNeeded();
	await expect(account).toBeVisible();
	expect((await account.boundingBox())!.height).toBeGreaterThanOrEqual(44);
	const line = officeLine(scope, office);
	await line.scrollIntoViewIfNeeded();
	await expect(line).toHaveAttribute('href', '/settings/offices');
	// At 200% text below 40rem (e.g., 960px at 32px root), SHELL-30 reflows the desktop sidebar to the 4rem icon rail.
	const railReflow = width >= 640 && width < 40 * 16 * scale;
	if (railReflow) {
		await expect(page.getByRole('complementary', { name: 'Sidebar', exact: true })).toHaveAttribute(
			'data-collapsed',
			'true'
		);
		await expect(line.locator('span')).toHaveCount(0);
	} else {
		const words = line.locator('span').last();
		await textFits(words);
		await page.mouse.move(width - 1, 0);
		const restingContrast = await contrast(words);
		expect(restingContrast).toBeGreaterThanOrEqual(4.5);
		await line.hover();
		expect(await contrast(words)).toBeGreaterThanOrEqual(4.5);
	}
	if (width < 640) expect((await line.boundingBox())!.height).toBeGreaterThanOrEqual(44);
	await page.keyboard.press('Tab');
	await line.focus();
	await expect(line).toBeFocused();
	await expect(line).toHaveCSS('outline-width', '2px');
	await expect(line).toHaveCSS('outline-style', 'solid');
	return line;
}

for (const width of [320, 960, 1280]) {
	test(
		`footer: Switch organization in the account menu opens the organizations page at ${width}px`,
		{ tag: '@webkit' },
		async ({ page }) => {
			await page.setViewportSize({ width, height: 900 });
			await openShell(page);
			const scope = await footerScope(page, width);
			await accountButton(scope).press('Enter');
			const menu = page.getByRole('menu');
			await expect(menu.getByRole('menuitem')).toHaveText([
				'Your preferences',
				'Keyboard shortcuts ?',
				'Switch organization',
				'About',
				'Sign out'
			]);
			await shot(page, `footer-account-menu-${width}`);
			await menu.getByRole('menuitem', { name: 'Switch organization', exact: true }).press('Enter');
			await expect(page).toHaveURL(/\/welcome$/);
			await expect(page.getByRole('heading', { name: 'Organizations', exact: true })).toBeVisible();
			await expect(
				page.getByRole('listitem').filter({ hasText: 'Footer reference' })
			).toBeVisible();
		}
	);
}

test(
	'footer: the collapsed rail keeps a 48 px account button, compact office status and all destinations',
	{
		tag: '@webkit'
	},
	async ({ page }) => {
		await page.setViewportSize({ width: 640, height: 900 });
		await openShell(page);
		await page.getByRole('button', { name: 'Auto-hide sidebar', exact: true }).click();
		await page.mouse.move(620, 20);
		const aside = page.getByRole('complementary', { name: 'Sidebar', exact: true });
		await expect(aside).toHaveAttribute('data-collapsed', 'true');
		await expect.poll(async () => Math.round((await aside.boundingBox())?.width ?? 0)).toBe(68);
		const account = page.getByRole('button', { name: 'You · Owner, account menu', exact: true });
		expect((await account.boundingBox())?.width).toBe(48);
		const compactOffice = officeLine(aside);
		await expect(compactOffice).toBeVisible();
		await expect(compactOffice.locator('span')).toHaveCount(0);
		const mainNav = page.getByRole('navigation', { name: 'Main', exact: true });
		await expect(mainNav.locator('ul:not([data-sidebar-favs]) > li > a')).toHaveCount(9);
		// Unpinned Organization items (Processes) stay reachable from the collapsed rail More menu.
		const railMore = page.getByTestId('organization-more-toggle-rail');
		await expect(railMore).toBeVisible();
		await shot(page, 'footer-rail');
		// Opening rail More in auto-hide rail mode stays open (does not expand/unmount) and hides its tooltip.
		await railMore.dispatchEvent('click');
		const processesItem = page.getByRole('menuitem', { name: 'Processes', exact: true });
		await expect(processesItem).toBeVisible();
		await expect(aside).toHaveAttribute('data-collapsed', 'true');
		await expect(
			page.getByRole('tooltip', { name: 'More organization pages', exact: true })
		).toHaveCount(0);
		await page.keyboard.press('Escape');
		await expect(processesItem).toHaveCount(0);
		await expect(aside).toHaveAttribute('data-collapsed', 'true');
		await expect(railMore).toBeFocused();
		// Pin the sidebar open, then enter 200%-text reflow rail so unzooming deterministically unmounts rail More while open.
		await page.setViewportSize({ width: 960, height: 900 });
		await page
			.getByRole('navigation', { name: 'Main', exact: true })
			.getByRole('link', { name: 'Home', exact: true })
			.hover();
		await expect(aside).not.toHaveAttribute('data-collapsed', 'true');
		await page.getByRole('button', { name: 'Pin sidebar open', exact: true }).click();
		await page.mouse.move(940, 20);
		const zoomTag = await page.addStyleTag({ content: 'html { font-size: 200% !important; }' });
		await expect(railMore).toBeVisible();
		await railMore.scrollIntoViewIfNeeded();
		await railMore.hover();
		await expect(
			page.getByRole('tooltip', { name: 'More organization pages', exact: true })
		).toBeVisible();
		await railMore.click();
		await expect(processesItem).toBeVisible();
		await expect(
			page.getByRole('tooltip', { name: 'More organization pages', exact: true })
		).toHaveCount(0);
		// Unzoom out of textReflowRail while rail More is open; menu unmounts cleanly and auto-hide still collapses after.
		await zoomTag.evaluate((el) => el.remove());
		await expect(processesItem).toHaveCount(0);
		await expect(aside).not.toHaveAttribute('data-collapsed', 'true');
		await page.getByRole('button', { name: 'Auto-hide sidebar', exact: true }).click();
		await page.mouse.move(940, 20);
		await expect(aside).toHaveAttribute('data-collapsed', 'true');
		await expect.poll(async () => Math.round((await aside.boundingBox())?.width ?? 0)).toBe(68);
		// Re-enter 200%-text reflow rail, select Processes, and confirm destination heading receives focus.
		await page.addStyleTag({ content: 'html { font-size: 200% !important; }' });
		await expect(railMore).toBeVisible();
		await expect(railMore).toHaveAttribute('aria-expanded', 'false');
		await railMore.click();
		await expect(processesItem).toBeVisible();
		await processesItem.click();
		await expect(page).toHaveURL(/\/settings\/processes$/);
		await expect(
			page
				.getByRole('main')
				.getByRole('heading', { level: 2, name: /^Processes/ })
				.first()
		).toBeFocused();
		// Re-opening rail More on the active route marks Processes with aria-current="page".
		await railMore.click();
		await expect(processesItem).toHaveAttribute('aria-current', 'page');
		await page.keyboard.press('Escape');
		await expect(processesItem).toHaveCount(0);
	}
);

const longHost = 'workstationwithaverylongname.local';
const longOffice = 'Internationalengineeringoffice';
async function longConnection(page: Page) {
	await page.evaluate(
		({ host, name }) => {
			// Test-owned browser storage only: seed the remembered connection and its matching mock office.
			const connections = JSON.parse(
				localStorage.getItem('agentic-org.mock.connections') ?? '[]'
			) as Connection[];
			const current = connections.find((c) => c.organization.name === 'Footer reference');
			if (!current) throw new Error('Expected the footer fixture connection');
			const office = { ...current.office, host, name };
			const organization = {
				...current.organization,
				offices: current.organization.offices.map((item) => (item.id === office.id ? office : item))
			};
			localStorage.setItem(
				'agentic-org.mock.connections',
				JSON.stringify([...connections.filter((c) => c !== current), { office, organization }])
			);
			const hosted = JSON.parse(
				localStorage.getItem('agentic-org.mock.organizations') ?? '[]'
			) as Organization[];
			localStorage.setItem(
				'agentic-org.mock.organizations',
				JSON.stringify(hosted.map((item) => (item.id === organization.id ? organization : item)))
			);
		},
		{ host: longHost, name: longOffice }
	);
	await visit(page, '/home');
}

for (const width of [320, 960, 1280]) {
	for (const scale of [1, 2]) {
		for (const long of [false, true]) {
			test(
				`footer: ${long ? 'a long office name' : 'current data'} fits and reads at ${width}px and ${scale * 100}% text`,
				{ tag: '@webkit' },
				async ({ page }) => {
					await page.setViewportSize({ width, height: 900 });
					await openShell(page);
					if (long) await longConnection(page);
					await page.evaluate((scale) => {
						document.documentElement.style.fontSize = `${scale * 100}%`;
					}, scale);
					const line = await footerChecks(page, width, scale, long ? longOffice : 'HQ');
					await shot(page, `footer-${width}-text${scale * 100}-${long ? 'long' : 'current'}`, {
						context: line
					});
				}
			);
		}
	}
}

for (const width of [320, 1280]) {
	test(
		`footer: the office line opens Offices at ${width}px`,
		{ tag: '@webkit' },
		async ({ page }) => {
			await page.setViewportSize({ width, height: 900 });
			await openShell(page);
			const scope = await footerScope(page, width);
			await officeLine(scope).click({ position: { x: 4, y: 4 } });
			await expect(page).toHaveURL(/\/settings\/offices$/);
		}
	);
}

for (const [path, label, state] of [
	['/teams/members?mock=empty', 'Teams', 'No members yet'],
	['/chat?chat=offline&c=c-general', 'Chat', undefined]
] as const) {
	test(`footer fixture: ${path} keeps the footer`, { tag: '@webkit' }, async ({ page }) => {
		await page.setViewportSize({ width: 320, height: 900 });
		await openShell(page);
		await visit(page, path);
		await expect(
			page.getByRole('button', { name: 'Menu', exact: true }).locator('..')
		).toContainText(label);
		if (state) await expect(page.getByRole('heading', { name: state, exact: true })).toBeVisible();
		else await expect(page.getByTestId('office-down')).toBeVisible();
		const line = await footerChecks(page, 320, 1);
		await shot(page, `footer-fixture-${label.toLowerCase()}`, { context: line });
	});
}

test('footer fixture: a stopped office says so', { tag: '@webkit' }, async ({ page }) => {
	await page.setViewportSize({ width: 1280, height: 900 });
	await openShell(page, '/create?office=crashed');
	await expect(officeLine(page.locator('body'))).toHaveAccessibleName('HQ office · Stopped');
	const line = await footerChecks(page, 1280, 1);
	await shot(page, 'footer-fixture-stopped', { context: line });
});

for (const width of [320, 1280]) {
	test(
		`footer paths: every shell destination keeps the footer at ${width}px`,
		{
			tag: '@webkit'
		},
		async ({ page }) => {
			test.setTimeout(120_000);
			await page.setViewportSize({ width, height: 900 });
			await openShell(page);
			for (const path of [
				'/home',
				'/inbox',
				'/projects',
				'/projects/beacon',
				'/work',
				'/chat',
				'/activity',
				'/teams',
				'/teams/members',
				'/settings',
				'/preferences'
			]) {
				await visit(page, path);
				const line = await footerChecks(page, width, 1);
				await shot(page, `footer-path-${path.slice(1).replaceAll('/', '-')}-${width}`, {
					context: line
				});
				if (width < 640) {
					await page.keyboard.press('Escape');
					await expect(page.getByRole('button', { name: 'Menu', exact: true })).toBeFocused();
				}
			}
		}
	);
}
