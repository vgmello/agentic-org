import { expect, test, type Page } from '@playwright/test';
import { visit } from './ready';
import { shot } from './shot';

async function createOrganization(page: Page) {
	await visit(page, '/create');
	await page.getByLabel('Organization name').fill('Acme');
	await page.getByRole('button', { name: 'Create organization' }).click();
	await expect(page).toHaveURL(/\/agents$/);
}

async function openOffices(page: Page) {
	await createOrganization(page);
	await page.getByRole('navigation', { name: 'Main', exact: true }).getByRole('link', { name: 'Offices', exact: true }).click();
	await expect(page.getByRole('heading', { name: 'Offices', exact: true })).toBeVisible();
}

for (const width of [960, 1440]) {
	test(`BUG-52: the full navigation remains at ${width}px`, async ({ page }) => {
		await openOffices(page);
		await page.setViewportSize({ width, height: 900 });
		const nav = page.getByRole('navigation', { name: 'Main', exact: true });
		expect((await nav.locator('..').boundingBox())?.width).toBe(288);
		await expect(nav.getByRole('link', { name: 'Offices', exact: true })).toHaveAttribute('aria-current', 'page');
		await expect(page.getByRole('button', { name: 'Menu', exact: true })).toHaveCount(0);
		await shot(page, `shell-full-${width}`);
	});
}

for (const width of [959, 720]) {
	test(`BUG-52: the icon rail keeps navigation names and tooltips at ${width}px`, async ({ page }) => {
		await openOffices(page);
		await page.setViewportSize({ width, height: width === 720 ? 450 : 900 });
		const nav = page.getByRole('navigation', { name: 'Main', exact: true });
		await shot(page, `shell-rail-${width}`);
		expect((await nav.locator('..').boundingBox())?.width).toBe(64);
		await expect(nav.getByRole('link', { name: 'Offices', exact: true })).toHaveAttribute('aria-current', 'page');
		for (const name of ['Chat', 'Agents', 'Org chart', 'Projects', 'Work', 'Sessions', 'Activity', 'Processes', 'Offices', 'Settings']) {
			await expect(nav.getByRole('link', { name, exact: true })).toBeVisible();
		}
		await expect(page.getByRole('link', { name: /^Switch organization/ }).locator('svg')).toBeVisible();
		const office = nav.getByRole('link', { name: 'Offices', exact: true });
		await office.hover();
		await expect(page.getByRole('tooltip', { name: 'Offices', exact: true })).toHaveText('Offices');
		await shot(page, `shell-rail-hover-${width}`);
		await office.focus();
		await page.keyboard.press('Tab');
		await expect(nav.getByRole('link', { name: 'Settings', exact: true })).toBeFocused();
		await expect(page.getByRole('tooltip', { name: 'Settings', exact: true })).toBeVisible();
		await expect(page.getByRole('tooltip', { name: 'Offices', exact: true })).not.toBeVisible();
		await shot(page, `shell-rail-focus-${width}`);
	});
}

for (const width of [639, 600, 320]) {
	test(`BUG-52: the mobile Menu traps and restores navigation focus at ${width}px`, async ({ page }) => {
		await openOffices(page);
		await page.setViewportSize({ width, height: 900 });
		const menu = page.getByRole('button', { name: 'Menu', exact: true });
		await shot(page, `shell-mobile-closed-${width}`);
		await expect(menu).toBeVisible();
		await expect(menu).toHaveAttribute('aria-expanded', 'false');
		await expect(menu).toHaveAttribute('aria-controls', /\S+/);
		expect(await page.getByRole('main').evaluate((el) => {
			const css = getComputedStyle(el);
			return el.clientWidth - parseFloat(css.paddingLeft) - parseFloat(css.paddingRight);
		})).toBeGreaterThanOrEqual(200);
		expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(width);
		expect((await page.locator('[data-shell-header]').boundingBox())?.height).toBe(56);
		await menu.click();
		const dialog = page.getByRole('dialog', { name: 'Main navigation', exact: true });
		await expect(dialog).toBeVisible();
		await expect(dialog).toHaveCSS('width', `${Math.min(288, width - 56)}px`);
		await expect(menu).toHaveAttribute('aria-expanded', 'true');
		await expect(dialog).toHaveAttribute('id', (await menu.getAttribute('aria-controls'))!);
		await expect(dialog.getByRole('link', { name: 'Offices', exact: true })).toBeFocused();
		await shot(page, `shell-mobile-open-${width}`);
		const focusables = await dialog.locator('a[href], button:not([disabled])').count();
		for (let i = 0; i < focusables + 2; i++) {
			await page.keyboard.press('Tab');
			expect(await dialog.evaluate((el) => el.contains(document.activeElement))).toBe(true);
		}
		await page.keyboard.press('Escape');
		await expect(dialog).not.toBeVisible();
		await expect(menu).toBeFocused();
		await menu.click();
		await dialog.getByRole('link', { name: 'Offices', exact: true }).click();
		await expect(page.getByRole('heading', { level: 1, name: 'Offices', exact: true })).toBeFocused();
		await expect(dialog).not.toBeVisible();
		await menu.click();
		await dialog.getByRole('link', { name: 'Settings', exact: true }).click();
		await expect(page).toHaveURL(/\/settings$/);
		await expect(dialog).not.toBeVisible();
		await expect(page.getByRole('heading', { level: 1, name: 'Settings', exact: true })).toBeFocused();
		await expect(page.getByRole('main')).toHaveCount(1);
		await expect(menu).toHaveAttribute('aria-expanded', 'false');
		await shot(page, `shell-mobile-destination-${width}`);
		await page.goBack();
		await expect(page).toHaveURL(/\/offices$/);
		await expect(page.getByRole('list', { name: 'Offices', exact: true })).toBeVisible();
		await page.goBack();
		await expect(page).toHaveURL(/\/agents$/);
	});
}

test('BUG-52: the scrim dismisses Menu and widening the window releases the focus trap', async ({ page }) => {
	await openOffices(page);
	await page.setViewportSize({ width: 600, height: 900 });
	const menu = page.getByRole('button', { name: 'Menu', exact: true });
	await menu.click();
	const dialog = page.getByRole('dialog', { name: 'Main navigation', exact: true });
	await expect(dialog).toBeVisible();
	await page.locator('[data-slot="sheet-overlay"]').click({ position: { x: 590, y: 120 } });
	await expect(dialog).not.toBeVisible();
	await expect(menu).toBeFocused();
	await menu.click();
	await expect(dialog).toBeVisible();
	await page.setViewportSize({ width: 720, height: 900 });
	await expect(dialog).not.toBeVisible();
	await expect(page.getByRole('navigation', { name: 'Main', exact: true }).getByRole('link', { name: 'Offices', exact: true })).toBeFocused();
	await expect(menu).toHaveCount(0);
	await shot(page, 'shell-menu-resized-to-rail');
});

for (const dismissal of ['Escape', 'scrim', 'Close']) {
	test(`BUG-59: pointer-open Menu restores focus after ${dismissal}`, { tag: '@webkit' }, async ({ page }) => {
		await openOffices(page);
		await page.setViewportSize({ width: 600, height: 900 });
		const menu = page.getByRole('button', { name: 'Menu', exact: true });
		await menu.click();
		const dialog = page.getByRole('dialog', { name: 'Main navigation' });
		await expect(dialog).toBeVisible();
		if (dismissal === 'Escape') await page.keyboard.press('Escape');
		else if (dismissal === 'Close') await dialog.getByRole('button', { name: 'Close' }).click();
		else await page.locator('[data-slot="sheet-overlay"]').click({ position: { x: 590, y: 120 } });
		await expect(dialog).not.toBeVisible();
		await expect(menu).toBeFocused();
		await shot(page, `shell-menu-return-${dismissal.toLowerCase()}`);
		await page.goBack();
		await expect(page).toHaveURL(/\/agents$/);
	});
}

test('BUG-59: browser Back dismisses Menu before leaving the page', { tag: ['@journey', '@webkit'] }, async ({ page }) => {
	await openOffices(page);
	await page.setViewportSize({ width: 600, height: 900 });
	const menu = page.getByRole('button', { name: 'Menu', exact: true });
	await menu.click();
	const dialog = page.getByRole('dialog', { name: 'Main navigation' });
	await expect(dialog).toBeVisible();
	await page.evaluate(() => window.history.back());
	await expect(dialog).not.toBeVisible();
	await expect(page).toHaveURL(/\/offices$/);
	await expect(page.getByRole('list', { name: 'Offices', exact: true })).toBeVisible();
	await expect(menu).toBeFocused();
	await shot(page, 'shell-menu-back-stays-on-page');
	await page.goBack();
	await expect(page).toHaveURL(/\/agents$/);
	await expect(page.getByRole('group', { name: 'Show agents' })).toBeVisible();
});

test('BUG-59: opening and dismissing Menu preserves Forward history', { tag: '@webkit' }, async ({ page }) => {
	await openOffices(page);
	await page.goBack();
	await expect(page).toHaveURL(/\/agents$/);
	await page.setViewportSize({ width: 600, height: 900 });
	const menu = page.getByRole('button', { name: 'Menu', exact: true });
	await menu.click();
	const dialog = page.getByRole('dialog', { name: 'Main navigation' });
	await expect(dialog).toBeVisible();
	await page.keyboard.press('Escape');
	await expect(dialog).not.toBeVisible();
	await expect(menu).toBeFocused();
	await page.goForward();
	await expect(page).toHaveURL(/\/offices$/);
	await expect(page.getByRole('list', { name: 'Offices', exact: true })).toBeVisible();
	await shot(page, 'shell-menu-keeps-forward-history');
});

// Older-browser fallback covers a native fragment observed growing history.
// Histories with unindexed entries and existing Forward entries use the Navigation API.
for (const legacy of [false, true]) {
	test(`BUG-59: Back dismisses Menu before fragment history${legacy ? ' without the Navigation API' : ''}`, { tag: '@webkit' }, async ({ page }) => {
		if (legacy) await page.addInitScript(() => Object.defineProperty(window, 'navigation', { value: undefined }));
		await openOffices(page);
		await page.setViewportSize({ width: 600, height: 900 });
		await page.evaluate(() => { window.location.hash = 'shell-page-title'; });
		await expect(page).toHaveURL(/\/offices#shell-page-title$/);
		const menu = page.getByRole('button', { name: 'Menu', exact: true });
		await menu.click();
		const dialog = page.getByRole('dialog', { name: 'Main navigation' });
		await expect(dialog).toBeVisible();
		await page.evaluate(() => window.history.back());
		await expect(dialog).not.toBeVisible();
		await expect(page).toHaveURL(/\/offices#shell-page-title$/);
		await expect(menu).toBeFocused();
		await shot(page, legacy ? 'shell-menu-back-fragment-legacy' : 'shell-menu-back-fragment');
		await menu.click();
		await expect(dialog).toBeVisible();
		await page.keyboard.press('Escape');
		await expect(dialog).not.toBeVisible();
		await expect(menu).toBeFocused();
		await expect(page).toHaveURL(/\/offices#shell-page-title$/);
		await page.goBack();
		await expect(page).toHaveURL(/\/offices$/);
	});
}

test('BUG-59: Back keeps the fragment reached by Forward while Menu is open', { tag: '@webkit' }, async ({ page }) => {
	await openOffices(page);
	await page.setViewportSize({ width: 600, height: 900 });
	for (const fragment of ['one', 'two']) {
		await page.evaluate((value) => { window.location.hash = value; }, fragment);
		await expect(page).toHaveURL(new RegExp(`#${fragment}$`));
	}
	await page.evaluate(() => window.history.back());
	await expect(page).toHaveURL(/#one$/);
	const menu = page.getByRole('button', { name: 'Menu', exact: true });
	await menu.click();
	const dialog = page.getByRole('dialog', { name: 'Main navigation' });
	await expect(dialog).toBeVisible();
	await page.evaluate(() => window.history.forward());
	await expect(page).toHaveURL(/#two$/);
	await expect(dialog).toBeVisible();
	await page.evaluate(() => window.history.back());
	await expect(dialog).not.toBeVisible();
	await expect(page).toHaveURL(/#two$/);
	await expect(menu).toBeFocused();
	await shot(page, 'shell-menu-back-after-forward');
});

test('BUG-59: fragment Forward stays on the next entry without the Navigation API', { tag: '@webkit' }, async ({ page }) => {
	await page.addInitScript(() => Object.defineProperty(window, 'navigation', { value: undefined }));
	await openOffices(page);
	await page.setViewportSize({ width: 600, height: 900 });
	for (const fragment of ['one', 'two', 'three']) {
		await page.evaluate((value) => { window.location.hash = value; }, fragment);
		await expect(page).toHaveURL(new RegExp(`#${fragment}$`));
	}
	await page.evaluate(() => window.history.go(-2));
	await expect(page).toHaveURL(/#one$/);
	await page.getByRole('button', { name: 'Menu', exact: true }).click();
	const dialog = page.getByRole('dialog', { name: 'Main navigation' });
	await expect(dialog).toBeVisible();
	await page.evaluate(() => window.history.forward());
	await expect(page).toHaveURL(/#two$/);
	await expect(dialog).toBeVisible();
	await shot(page, 'shell-menu-fragment-forward');
	await expect(page).toHaveURL(/#two$/);
});

test('BUG-59: Back dismisses Menu before a native fragment entry on a previous route', { tag: '@webkit' }, async ({ page }) => {
	await createOrganization(page);
	await page.evaluate(() => { window.location.hash = 'shell-page-title'; });
	await expect(page).toHaveURL(/\/agents#shell-page-title$/);
	await page.getByRole('navigation', { name: 'Main', exact: true }).getByRole('link', { name: 'Offices', exact: true }).click();
	await expect(page.getByRole('list', { name: 'Offices', exact: true })).toBeVisible();
	await page.setViewportSize({ width: 600, height: 900 });
	const menu = page.getByRole('button', { name: 'Menu', exact: true });
	await menu.click();
	const dialog = page.getByRole('dialog', { name: 'Main navigation' });
	await expect(dialog).toBeVisible();
	await page.evaluate(() => window.history.back());
	await expect(dialog).not.toBeVisible();
	await expect(page).toHaveURL(/\/offices$/);
	await expect(page.getByRole('list', { name: 'Offices', exact: true })).toBeVisible();
	await expect(menu).toBeFocused();
	await expect(page.getByRole('heading', { level: 1, name: 'Offices', exact: true })).toBeVisible();
	await shot(page, 'shell-menu-back-to-previous-route-fragment');
});

test('BUG-59: Back across two entries preserves the drawer opener route', { tag: '@webkit' }, async ({ page }) => {
	await createOrganization(page);
	await page.evaluate(() => { window.location.hash = 'shell-page-title'; });
	await expect(page).toHaveURL(/\/agents#shell-page-title$/);
	await page.getByRole('navigation', { name: 'Main', exact: true }).getByRole('link', { name: 'Offices', exact: true }).click();
	await expect(page.getByRole('heading', { name: 'Offices', exact: true })).toBeVisible();
	await page.setViewportSize({ width: 600, height: 900 });
	const menu = page.getByRole('button', { name: 'Menu', exact: true });
	await menu.click();
	const dialog = page.getByRole('dialog', { name: 'Main navigation' });
	await expect(dialog).toBeVisible();
	await page.evaluate(() => window.history.go(-2));
	await expect(dialog).not.toBeVisible();
	await expect(page).toHaveURL(/\/offices$/);
	await expect(page.getByRole('heading', { level: 1, name: 'Offices', exact: true })).toBeVisible();
	await expect(page.getByRole('list', { name: 'Offices', exact: true })).toBeVisible();
	await expect(menu).toBeFocused();
	await shot(page, 'shell-menu-back-across-two-entries');
});

test('BUG-59: Forward can reach an unfragmented entry without the Navigation API', { tag: '@webkit' }, async ({ page }) => {
	await page.addInitScript(() => Object.defineProperty(window, 'navigation', { value: undefined }));
	await openOffices(page);
	await page.evaluate(() => { window.location.hash = 'shell-page-title'; });
	await expect(page).toHaveURL(/\/offices#shell-page-title$/);
	await page.setViewportSize({ width: 600, height: 900 });
	const menu = page.getByRole('button', { name: 'Menu', exact: true });
	const dialog = page.getByRole('dialog', { name: 'Main navigation' });
	await menu.click();
	await dialog.getByRole('link', { name: 'Offices', exact: true }).click();
	await expect(page).toHaveURL(/\/offices$/);
	await menu.click();
	await dialog.getByRole('link', { name: 'Settings', exact: true }).click();
	await expect(page).toHaveURL(/\/settings$/);
	await page.evaluate(() => window.history.go(-2));
	await expect(page).toHaveURL(/\/offices#shell-page-title$/);
	await menu.click();
	await expect(dialog).toBeVisible();
	await page.evaluate(() => window.history.forward());
	await expect(page).toHaveURL(/\/offices$/);
	await expect(page.getByRole('list', { name: 'Offices', exact: true })).toBeVisible();
	await shot(page, 'shell-menu-forward-to-unfragmented-entry');
	await expect(page).toHaveURL(/\/offices$/);
});

test('BUG-59: the focused rail tooltip survives keyboard scrolling', { tag: '@webkit' }, async ({ page, browserName }) => {
	await page.setViewportSize({ width: 720, height: 450 });
	await createOrganization(page);
	await visit(page, '/offices');
	const nav = page.getByRole('navigation', { name: 'Main', exact: true });
	const rail = nav.locator('..');
	await nav.getByRole('link', { name: 'Chat', exact: true }).focus();
	await expect(page.getByRole('tooltip', { name: 'Chat', exact: true })).toBeVisible();
	const initial = await rail.evaluate((el) => el.scrollTop);
	const key = browserName === 'webkit' ? 'Alt+Tab' : 'Tab';
	for (const name of ['Agents', 'Org chart', 'Projects', 'Work', 'Sessions', 'Activity']) {
		await page.keyboard.press(key);
		await expect(nav.getByRole('link', { name, exact: true })).toBeFocused();
		if (name !== 'Activity') await expect(page.getByRole('tooltip', { name, exact: true })).toBeVisible();
	}
	await expect.poll(() => rail.evaluate((el) => el.scrollTop)).toBeGreaterThan(initial);
	await page.evaluate(() => new Promise<void>((resolve) => {
		requestAnimationFrame(() => requestAnimationFrame(() => resolve()));
	}));
	await shot(page, 'shell-rail-keyboard-scroll', { settle: false });
	const tooltip = page.getByRole('tooltip', { name: 'Activity', exact: true });
	await expect(tooltip).toBeVisible();
	await page.keyboard.press('Escape');
	await expect(tooltip).not.toBeVisible();
	await expect(nav.getByRole('link', { name: 'Activity', exact: true })).toBeFocused();
});

test('BUG-59: a supplied navigation badge appears in the full sidebar and rail', { tag: '@webkit' }, async ({ page }) => {
	await visit(page, '/create');
	await page.evaluate(async () => {
		const path = '/src/lib/nav.ts';
		const { NAV } = await import(path);
		NAV.find((item: { href: string }) => item.href === '/chat').badge = { text: '3', label: '3 need you' };
	});
	await page.getByLabel('Organization name').fill('Acme');
	await page.getByRole('button', { name: 'Create organization' }).click();
	await expect(page).toHaveURL(/\/agents$/);
	const nav = page.getByRole('navigation', { name: 'Main', exact: true });
	const chat = nav.getByRole('link', { name: 'Chat', exact: true });
	const badge = chat.getByText('3', { exact: true });
	await expect(chat).toHaveAccessibleDescription('3 need you');
	await expect(badge).toBeInViewport({ ratio: 1 });
	await shot(page, 'shell-badge-full');
	await page.setViewportSize({ width: 720, height: 900 });
	await expect(badge).toBeInViewport({ ratio: 1 });
	await chat.focus();
	await expect(page.getByRole('tooltip', { name: 'Chat', exact: true })).toBeVisible();
	await expect(chat).toHaveAccessibleDescription(/3 need you/);
	await shot(page, 'shell-badge-rail');
});

test('BUG-59: Back with Menu open preserves an unsent Chat draft', { tag: '@webkit' }, async ({ page }) => {
	await createOrganization(page);
	await page.getByRole('navigation', { name: 'Main', exact: true }).getByRole('link', { name: 'Chat', exact: true }).click();
	await expect(page).toHaveTitle('Needs you · Chat · Agentic Org');
	await page.evaluate(() => { window.location.hash = 'shell-page-title'; });
	await expect(page).toHaveURL(/\/chat#shell-page-title$/);
	await page.getByRole('navigation', { name: 'Chat', exact: true })
		.getByTestId('conversation-link').filter({ hasText: 'general' }).first().click();
	const composer = page.getByTestId('composer');
	await expect(page).toHaveURL(/c=c-general/);
	await composer.fill('Keep this unsent draft');
	await page.setViewportSize({ width: 600, height: 900 });
	const menu = page.getByRole('button', { name: 'Menu', exact: true });
	await menu.click();
	const dialog = page.getByRole('dialog', { name: 'Main navigation' });
	await expect(dialog).toBeVisible();
	await page.evaluate(() => window.history.back());
	await expect(dialog).not.toBeVisible();
	await expect(page).toHaveURL(/c=c-general/);
	await expect(menu).toBeFocused();
	await expect(composer).toHaveValue('Keep this unsent draft');
	await shot(page, 'shell-menu-back-preserves-chat-draft');
	await page.evaluate(() => window.history.back());
	await expect(page).toHaveURL(/\/chat#shell-page-title$/);
	await expect(page).toHaveTitle('Needs you · Chat · Agentic Org');
	await expect(composer).toHaveCount(0);
});

test('BUG-59: Back dismisses Menu during an unfinished Forward', { tag: '@webkit' }, async ({ page }) => {
	await createOrganization(page);
	const mainNav = page.getByRole('navigation', { name: 'Main', exact: true });
	await mainNav.getByRole('link', { name: 'Projects', exact: true }).click();
	await expect(page).toHaveURL(/\/projects$/);
	await page.goBack();
	await expect(page.getByRole('group', { name: 'Show agents' })).toBeVisible();
	await page.reload();
	await page.locator('html[data-hydrated="true"]').waitFor();
	await expect(page.getByRole('group', { name: 'Show agents' })).toBeVisible();
	await page.evaluate(async () => {
		const path = '/e2e/fakes/org-api.ts';
		const { orgApi } = await import(path);
		const original = orgApi.view;
		orgApi.view = async (...args: unknown[]) => {
			orgApi.view = original;
			document.documentElement.dataset.forwardLoading = 'true';
			await new Promise<void>((resolve) => window.addEventListener('release-forward-load', () => resolve(), { once: true }));
			return original(...args);
		};
	});
	await page.setViewportSize({ width: 600, height: 900 });
	const menu = page.getByRole('button', { name: 'Menu', exact: true });
	await menu.click();
	const dialog = page.getByRole('dialog', { name: 'Main navigation' });
	await expect(dialog).toBeVisible();
	await page.evaluate(() => window.history.forward());
	await expect(page.locator('html')).toHaveAttribute('data-forward-loading', 'true');
	await expect(page).toHaveURL(/\/projects$/);
	try {
		await page.evaluate(() => window.history.back());
		await expect(dialog).not.toBeVisible();
		await expect(page).toHaveURL(/\/projects$/);
		await expect(menu).toBeFocused();
	} finally {
		await page.evaluate(() => window.dispatchEvent(new Event('release-forward-load')));
	}
	await expect(page.getByRole('heading', { level: 1, name: 'Projects', exact: true })).toBeVisible();
	await shot(page, 'shell-menu-back-during-forward');
});
