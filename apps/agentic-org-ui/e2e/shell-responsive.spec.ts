import { expect, test, type Page } from '@playwright/test';
import { visit } from './ready';
import { shot } from './shot';
import { enterMockedOrganization } from './mock-organization';

async function createOrganization(page: Page) {
	await enterMockedOrganization(page);
	await expect(page).toHaveURL(/\/teams\/members$/);
}

async function openActivity(page: Page) {
	await createOrganization(page);
	await page
		.getByRole('navigation', { name: 'Main', exact: true })
		.getByRole('link', { name: 'Activity', exact: true })
		.click();
	await expect(
		page.getByRole('heading', { level: 1, name: 'Activity', exact: true })
	).toBeVisible();
}

for (const width of [960, 1440]) {
	test(`BUG-52: the full navigation remains at ${width}px`, async ({ page }) => {
		await openActivity(page);
		await page.setViewportSize({ width, height: 900 });
		const nav = page.getByRole('navigation', { name: 'Main', exact: true });
		expect((await nav.locator('..').boundingBox())?.width).toBe(264);
		await expect(nav.getByRole('link', { name: 'Activity', exact: true })).toHaveAttribute(
			'aria-current',
			'page'
		);
		await expect(page.getByRole('button', { name: 'Menu', exact: true })).toHaveCount(0);
		await shot(page, `shell-full-${width}`);
	});
}

async function collapseToRail(page: Page, width: number) {
	await page.getByRole('button', { name: 'Auto-hide sidebar', exact: true }).click();
	await page.mouse.move(width - 20, 20);
	const aside = page.getByRole('complementary', { name: 'Sidebar', exact: true });
	await expect(aside).toHaveAttribute('data-collapsed', 'true');
	await expect.poll(async () => Math.round((await aside.boundingBox())?.width ?? 0)).toBe(68);
	return aside;
}

for (const width of [959, 720]) {
	test(`BUG-52: the collapsed icon rail keeps navigation names and tooltips at ${width}px`, async ({
		page,
		browserName
	}) => {
		await openActivity(page);
		await page.setViewportSize({ width, height: width === 720 ? 450 : 900 });
		const nav = page.getByRole('navigation', { name: 'Main', exact: true });
		const aside = await collapseToRail(page, width);
		await shot(page, `shell-rail-${width}`);
		expect(Math.round((await aside.boundingBox())?.width ?? 0)).toBe(68);
		await expect(nav.getByRole('link', { name: 'Activity', exact: true })).toHaveAttribute(
			'aria-current',
			'page'
		);
		for (const name of [
			'Home',
			'Work',
			'Chat',
			'Activity',
			'Projects',
			'Insights',
			'Teams',
			'Settings'
		]) {
			const link = nav.getByRole('link', { name, exact: true });
			await expect(link).toBeVisible();
			await expect(link).not.toHaveAttribute('type', 'button');
		}
		const account = page.getByRole('button', { name: 'You · Owner, account menu', exact: true });
		await expect(account.locator('svg')).toBeVisible();
		const activity = nav.getByRole('link', { name: 'Activity', exact: true });
		const activityLabel = activity.locator('span');
		await expect(activityLabel).toHaveClass(/sr-only/);
		// Hovering a nav link in the collapsed rail expands it to 264px and visibly reveals its text label.
		await activity.hover();
		await expect(aside).not.toHaveAttribute('data-collapsed', 'true');
		await expect.poll(async () => Math.round((await aside.boundingBox())?.width ?? 0)).toBe(264);
		await expect(activityLabel).toHaveText('Activity');
		await expect(activityLabel).not.toHaveClass(/sr-only/);
		await expect(activityLabel).toBeVisible();
		await shot(page, `shell-rail-hover-${width}`);
		// Moving away collapses back to 68px (hiding text labels to sr-only), and real keyboard Tab navigation expands it back to 264px.
		await page.mouse.move(width - 20, 20);
		await expect(aside).toHaveAttribute('data-collapsed', 'true');
		await expect.poll(async () => Math.round((await aside.boundingBox())?.width ?? 0)).toBe(68);
		await expect(activityLabel).toHaveClass(/sr-only/);
		await page.evaluate(() => {
			// Reset sequential focus navigation to the start of the document (HTML §6.6.3);
			// plain blur() on body leaves Chromium's starting point on the last interacted node.
			document.body.tabIndex = -1;
			document.body.focus({ preventScroll: true });
			document.body.blur();
			document.body.removeAttribute('tabindex');
		});
		const tabKey = browserName === 'webkit' && process.platform === 'darwin' ? 'Alt+Tab' : 'Tab';
		await page.keyboard.press(tabKey);
		await expect(aside).not.toHaveAttribute('data-collapsed', 'true');
		await expect.poll(async () => Math.round((await aside.boundingBox())?.width ?? 0)).toBe(264);
		for (
			let i = 0;
			i < 16 && !(await activity.evaluate((el) => el === document.activeElement));
			i++
		) {
			await page.keyboard.press(tabKey);
		}
		await expect(activity).toBeFocused();
		await expect(activityLabel).toHaveText('Activity');
		await expect(activityLabel).not.toHaveClass(/sr-only/);
		await expect(activityLabel).toBeVisible();
		await shot(page, `shell-rail-focus-${width}`);
	});
}

for (const width of [639, 600, 320]) {
	test(`BUG-52: the mobile Menu traps and restores navigation focus at ${width}px`, async ({
		page
	}) => {
		await openActivity(page);
		await page.setViewportSize({ width, height: 900 });
		const menu = page.getByRole('button', { name: 'Menu', exact: true });
		await shot(page, `shell-mobile-closed-${width}`);
		await expect(menu).toBeVisible();
		await expect(menu).toHaveAttribute('aria-expanded', 'false');
		await expect(menu).toHaveAttribute('aria-controls', /\S+/);
		expect(
			await page.getByRole('main').evaluate((el) => {
				const css = getComputedStyle(el);
				return el.clientWidth - parseFloat(css.paddingLeft) - parseFloat(css.paddingRight);
			})
		).toBeGreaterThanOrEqual(200);
		expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(
			width
		);
		expect((await page.locator('header.exec-topbar').boundingBox())?.height).toBeGreaterThanOrEqual(
			44
		);
		await menu.click();
		const dialog = page.getByRole('dialog', { name: 'Main navigation', exact: true });
		await expect(dialog).toBeVisible();
		await expect(dialog).toHaveCSS('width', `${Math.min(288, width - 56)}px`);
		await expect(menu).toHaveAttribute('aria-expanded', 'true');
		await expect(dialog).toHaveAttribute('id', (await menu.getAttribute('aria-controls'))!);
		await expect(dialog.getByRole('link', { name: 'Activity', exact: true })).toBeFocused();
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
		await dialog.getByRole('link', { name: 'Activity', exact: true }).click();
		await expect(
			page.getByRole('heading', { level: 1, name: 'Activity', exact: true })
		).toBeFocused();
		await expect(dialog).not.toBeVisible();
		await menu.click();
		await dialog.getByRole('link', { name: 'Work', exact: true }).click();
		await expect(page).toHaveURL(/\/work$/);
		await expect(dialog).not.toBeVisible();
		await expect(page.getByRole('heading', { level: 1, name: 'Work', exact: true })).toBeFocused();
		await expect(page.getByRole('main')).toHaveCount(1);
		await expect(menu).toHaveAttribute('aria-expanded', 'false');
		await shot(page, `shell-mobile-destination-${width}`);
		await page.goBack();
		await expect(page).toHaveURL(/\/activity$/);
		await expect(
			page.getByRole('heading', { level: 1, name: 'Activity', exact: true })
		).toBeVisible();
		await page.goBack();
		await expect(page).toHaveURL(/\/(agents|teams\/members)$/);
	});
}

test('BUG-52: the scrim dismisses Menu and widening the window releases the focus trap', async ({
	page
}) => {
	await openActivity(page);
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
	await expect(
		page
			.getByRole('navigation', { name: 'Main', exact: true })
			.getByRole('link', { name: 'Activity', exact: true })
	).toBeFocused();
	await expect(menu).toHaveCount(0);
	await shot(page, 'shell-menu-resized-to-rail');
});

for (const dismissal of ['Escape', 'scrim', 'Close']) {
	test(
		`BUG-59: pointer-open Menu restores focus after ${dismissal}`,
		{ tag: '@webkit' },
		async ({ page }) => {
			await openActivity(page);
			await page.setViewportSize({ width: 600, height: 900 });
			const menu = page.getByRole('button', { name: 'Menu', exact: true });
			await menu.click();
			const dialog = page.getByRole('dialog', { name: 'Main navigation' });
			await expect(dialog).toBeVisible();
			if (dismissal === 'Escape') await page.keyboard.press('Escape');
			else if (dismissal === 'Close') await dialog.getByRole('button', { name: 'Close' }).click();
			else
				await page.locator('[data-slot="sheet-overlay"]').click({ position: { x: 590, y: 120 } });
			await expect(dialog).not.toBeVisible();
			await expect(menu).toBeFocused();
			await shot(page, `shell-menu-return-${dismissal.toLowerCase()}`);
			await page.goBack();
			await expect(page).toHaveURL(/\/(agents|teams\/members)$/);
		}
	);
}

test(
	'BUG-59: browser Back dismisses Menu before leaving the page',
	{ tag: ['@journey', '@webkit'] },
	async ({ page }) => {
		await openActivity(page);
		await page.setViewportSize({ width: 600, height: 900 });
		const menu = page.getByRole('button', { name: 'Menu', exact: true });
		await menu.click();
		const dialog = page.getByRole('dialog', { name: 'Main navigation' });
		await expect(dialog).toBeVisible();
		await page.evaluate(() => window.history.back());
		await expect(dialog).not.toBeVisible();
		await expect(page).toHaveURL(/\/activity$/);
		await expect(
			page.getByRole('heading', { level: 1, name: 'Activity', exact: true })
		).toBeVisible();
		await expect(menu).toBeFocused();
		await shot(page, 'shell-menu-back-stays-on-page');
		await page.goBack();
		await expect(page).toHaveURL(/\/(agents|teams\/members)$/);
		await expect(page.getByRole('group', { name: 'Show members', exact: true })).toBeVisible();
	}
);

test(
	'BUG-59: opening and dismissing Menu preserves Forward history',
	{ tag: '@webkit' },
	async ({ page }) => {
		await openActivity(page);
		await page.goBack();
		await expect(page).toHaveURL(/\/(agents|teams\/members)$/);
		await page.setViewportSize({ width: 600, height: 900 });
		const menu = page.getByRole('button', { name: 'Menu', exact: true });
		await menu.click();
		const dialog = page.getByRole('dialog', { name: 'Main navigation' });
		await expect(dialog).toBeVisible();
		await page.keyboard.press('Escape');
		await expect(dialog).not.toBeVisible();
		await expect(menu).toBeFocused();
		await page.goForward();
		await expect(page).toHaveURL(/\/activity$/);
		await expect(
			page.getByRole('heading', { level: 1, name: 'Activity', exact: true })
		).toBeVisible();
		await shot(page, 'shell-menu-keeps-forward-history');
	}
);

// Older-browser fallback covers a native fragment observed growing history.
// Histories with unindexed entries and existing Forward entries use the Navigation API.
for (const legacy of [false, true]) {
	test(
		`BUG-59: Back dismisses Menu before fragment history${legacy ? ' without the Navigation API' : ''}`,
		{ tag: '@webkit' },
		async ({ page }) => {
			if (legacy)
				await page.addInitScript(() =>
					Object.defineProperty(window, 'navigation', { value: undefined })
				);
			await openActivity(page);
			await page.setViewportSize({ width: 600, height: 900 });
			await page.evaluate(() => {
				window.location.hash = 'shell-page-title';
			});
			await expect(page).toHaveURL(/\/activity#shell-page-title$/);
			const menu = page.getByRole('button', { name: 'Menu', exact: true });
			await menu.click();
			const dialog = page.getByRole('dialog', { name: 'Main navigation' });
			await expect(dialog).toBeVisible();
			await page.evaluate(() => window.history.back());
			await expect(dialog).not.toBeVisible();
			await expect(page).toHaveURL(/\/activity#shell-page-title$/);
			await expect(menu).toBeFocused();
			await shot(page, legacy ? 'shell-menu-back-fragment-legacy' : 'shell-menu-back-fragment');
			await menu.click();
			await expect(dialog).toBeVisible();
			await page.keyboard.press('Escape');
			await expect(dialog).not.toBeVisible();
			await expect(menu).toBeFocused();
			await expect(page).toHaveURL(/\/activity#shell-page-title$/);
			await page.goBack();
			await expect(page).toHaveURL(/\/activity$/);
		}
	);
}

test(
	'BUG-59: Back keeps the fragment reached by Forward while Menu is open',
	{ tag: '@webkit' },
	async ({ page }) => {
		await openActivity(page);
		await page.setViewportSize({ width: 600, height: 900 });
		for (const fragment of ['one', 'two']) {
			await page.evaluate((value) => {
				window.location.hash = value;
			}, fragment);
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
	}
);

test(
	'BUG-59: fragment Forward stays on the next entry without the Navigation API',
	{ tag: '@webkit' },
	async ({ page }) => {
		await page.addInitScript(() =>
			Object.defineProperty(window, 'navigation', { value: undefined })
		);
		await openActivity(page);
		await page.setViewportSize({ width: 600, height: 900 });
		for (const fragment of ['one', 'two', 'three']) {
			await page.evaluate((value) => {
				window.location.hash = value;
			}, fragment);
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
	}
);

test(
	'BUG-59: Back dismisses Menu before a native fragment entry on a previous route',
	{ tag: '@webkit' },
	async ({ page }) => {
		await createOrganization(page);
		await page.evaluate(() => {
			window.location.hash = 'shell-page-title';
		});
		await expect(page).toHaveURL(/\/(agents|teams\/members)#shell-page-title$/);
		await page
			.getByRole('navigation', { name: 'Main', exact: true })
			.getByRole('link', { name: 'Activity', exact: true })
			.click();
		await expect(
			page.getByRole('heading', { level: 1, name: 'Activity', exact: true })
		).toBeVisible();
		await page.setViewportSize({ width: 600, height: 900 });
		const menu = page.getByRole('button', { name: 'Menu', exact: true });
		await menu.click();
		const dialog = page.getByRole('dialog', { name: 'Main navigation' });
		await expect(dialog).toBeVisible();
		await page.evaluate(() => window.history.back());
		await expect(dialog).not.toBeVisible();
		await expect(page).toHaveURL(/\/activity$/);
		await expect(
			page.getByRole('heading', { level: 1, name: 'Activity', exact: true })
		).toBeVisible();
		await expect(menu).toBeFocused();
		await expect(
			page.getByRole('heading', { level: 1, name: 'Activity', exact: true })
		).toBeVisible();
		await shot(page, 'shell-menu-back-to-previous-route-fragment');
	}
);

test(
	'BUG-59: Back across two entries preserves the drawer opener route',
	{ tag: '@webkit' },
	async ({ page }) => {
		await createOrganization(page);
		await page.evaluate(() => {
			window.location.hash = 'shell-page-title';
		});
		await expect(page).toHaveURL(/\/(agents|teams\/members)#shell-page-title$/);
		await page
			.getByRole('navigation', { name: 'Main', exact: true })
			.getByRole('link', { name: 'Activity', exact: true })
			.click();
		await expect(
			page.getByRole('heading', { level: 1, name: 'Activity', exact: true })
		).toBeVisible();
		await page.setViewportSize({ width: 600, height: 900 });
		const menu = page.getByRole('button', { name: 'Menu', exact: true });
		await menu.click();
		const dialog = page.getByRole('dialog', { name: 'Main navigation' });
		await expect(dialog).toBeVisible();
		await page.evaluate(() => window.history.go(-2));
		await expect(dialog).not.toBeVisible();
		await expect(page).toHaveURL(/\/activity$/);
		await expect(
			page.getByRole('heading', { level: 1, name: 'Activity', exact: true })
		).toBeVisible();
		await expect(
			page.getByRole('heading', { level: 1, name: 'Activity', exact: true })
		).toBeVisible();
		await expect(menu).toBeFocused();
		await shot(page, 'shell-menu-back-across-two-entries');
	}
);

test(
	'BUG-59: Forward can reach an unfragmented entry without the Navigation API',
	{ tag: '@webkit' },
	async ({ page }) => {
		await page.addInitScript(() =>
			Object.defineProperty(window, 'navigation', { value: undefined })
		);
		await openActivity(page);
		await page.evaluate(() => {
			window.location.hash = 'shell-page-title';
		});
		await expect(page).toHaveURL(/\/activity#shell-page-title$/);
		await page.setViewportSize({ width: 600, height: 900 });
		const menu = page.getByRole('button', { name: 'Menu', exact: true });
		const dialog = page.getByRole('dialog', { name: 'Main navigation' });
		await menu.click();
		await dialog.getByRole('link', { name: 'Activity', exact: true }).click();
		await expect(page).toHaveURL(/\/activity$/);
		await menu.click();
		await dialog.getByRole('link', { name: 'Settings', exact: true }).click();
		await expect(page).toHaveURL(/\/settings$/);
		await page.evaluate(() => window.history.go(-2));
		await expect(page).toHaveURL(/\/activity#shell-page-title$/);
		await menu.click();
		await expect(dialog).toBeVisible();
		await page.evaluate(() => window.history.forward());
		await expect(page).toHaveURL(/\/activity$/);
		await expect(
			page.getByRole('heading', { level: 1, name: 'Activity', exact: true })
		).toBeVisible();
		await shot(page, 'shell-menu-forward-to-unfragmented-entry');
		await expect(page).toHaveURL(/\/activity$/);
	}
);

test(
	'BUG-59: keyboard traversal scrolls the short sidebar navigation while keeping focus visible',
	{ tag: '@webkit' },
	async ({ page, browserName }) => {
		await page.setViewportSize({ width: 720, height: 450 });
		await createOrganization(page);
		await visit(page, '/activity');
		const nav = page.getByRole('navigation', { name: 'Main', exact: true });
		// The navigation list scrolls inside the sidebar; the logo and the footer stay put.
		await nav.getByRole('link', { name: 'Home', exact: true }).focus();
		const initial = await nav.evaluate((el) => el.scrollTop);
		const key = browserName === 'webkit' && process.platform === 'darwin' ? 'Alt+Tab' : 'Tab';
		const tabToNavLink = async (name: string) => {
			const target = nav.getByRole('link', {
				name: name === 'Inbox' ? /^Inbox/ : name,
				exact: name !== 'Inbox'
			});
			for (let step = 0; step < 6; step++) {
				await page.keyboard.press(key);
				if (await target.evaluate((el) => el === document.activeElement)) return;
			}
			await expect(target).toBeFocused();
		};
		for (const name of ['Inbox', 'Work', 'Chat', 'Activity', 'Projects', 'Insights']) {
			await tabToNavLink(name);
		}
		const favToggle = nav.getByRole('button', { name: 'Collapse favorite projects', exact: true });
		if (await favToggle.isVisible()) {
			for (let step = 0; step < 4; step++) {
				await page.keyboard.press(key);
				if (await favToggle.evaluate((el) => el === document.activeElement)) break;
			}
			await expect(favToggle).toBeFocused();
			const favLinks = nav.locator('[data-sidebar-favs] a');
			const favCount = await favLinks.count();
			for (let i = 0; i < favCount; i++) {
				for (let step = 0; step < 3; step++) {
					await page.keyboard.press(key);
					if (await favLinks.nth(i).evaluate((el) => el === document.activeElement)) break;
				}
				await expect(favLinks.nth(i)).toBeFocused();
			}
		}
		for (const name of ['Teams', 'Settings']) {
			await tabToNavLink(name);
		}
		const settingsLink = nav.getByRole('link', { name: 'Settings', exact: true });
		await expect.poll(() => nav.evaluate((el) => el.scrollTop)).toBeGreaterThan(initial);
		await expect(settingsLink).toBeInViewport();
		await shot(page, 'shell-rail-keyboard-scroll', { settle: false });
		// In collapsed 68px rail mode, the account menu button opens its accessible tooltip and Escape dismisses it.
		await collapseToRail(page, 720);
		const account = page.getByRole('button', { name: 'You · Owner, account menu', exact: true });
		await account.focus();
		await expect(account).toBeFocused();
		const tooltip = page.getByRole('tooltip', { name: 'You · Owner', exact: true });
		await expect(tooltip).toBeVisible();
		await page.keyboard.press('Escape');
		await expect(tooltip).not.toBeVisible();
		await expect(account).toBeFocused();
	}
);

test(
	'BUG-59: the live Inbox badge appears in the full sidebar and collapsed rail',
	{ tag: '@webkit' },
	async ({ page }) => {
		await enterMockedOrganization(page);
		await expect(page).toHaveURL(/\/teams\/members$/);
		const nav = page.getByRole('navigation', { name: 'Main', exact: true });
		const inboxName = /^Inbox(?:\s*,\s*|\s+)3 (?:waiting on you|needing attention)$/i;
		const inbox = nav.getByRole('link', { name: inboxName });
		await expect(inbox).toBeVisible();
		const dot = inbox.locator('.exec-nav-dot');
		await expect(dot).toBeInViewport({ ratio: 1 });
		await shot(page, 'shell-badge-full');
		await page.setViewportSize({ width: 720, height: 900 });
		await collapseToRail(page, 720);
		const railInbox = nav.getByRole('link', { name: inboxName });
		await expect(railInbox).toBeVisible();
		await expect(railInbox.locator('.exec-nav-dot')).toBeInViewport({ ratio: 1 });
		await railInbox.focus();
		await expect(
			page.getByRole('tooltip', { name: /Inbox \(3 needing attention\)/ })
		).toBeVisible();
		await page.keyboard.press('Escape');
		await shot(page, 'shell-badge-rail');
	}
);

test(
	'BUG-59: Back with Menu open preserves an unsent Chat draft',
	{ tag: '@webkit' },
	async ({ page }) => {
		await createOrganization(page);
		await page
			.getByRole('navigation', { name: 'Main', exact: true })
			.getByRole('link', { name: 'Chat', exact: true })
			.click();
		await page
			.getByRole('navigation', { name: 'Chat', exact: true })
			.getByRole('button', { name: /Mentions/ })
			.click();
		await expect(page).toHaveTitle('Mentions · Chat · Agentic Org');
		await page.evaluate(() => {
			window.location.hash = 'shell-page-title';
		});
		await expect(page).toHaveURL(/\/chat\?v=mentions#shell-page-title$/);
		await page
			.getByRole('navigation', { name: 'Chat', exact: true })
			.getByTestId('conversation-link')
			.filter({ hasText: 'general' })
			.first()
			.click();
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
		await expect(page).toHaveURL(/\/chat\?v=mentions#shell-page-title$/);
		await expect(page).toHaveTitle('Mentions · Chat · Agentic Org');
		await expect(composer).toHaveCount(0);
	}
);

test(
	'BUG-59: Back dismisses Menu during an unfinished Forward',
	{ tag: '@webkit' },
	async ({ page }) => {
		await createOrganization(page);
		const mainNav = page.getByRole('navigation', { name: 'Main', exact: true });
		await mainNav.getByRole('link', { name: 'Projects', exact: true }).click();
		await expect(page).toHaveURL(/\/projects$/);
		await page.goBack();
		await expect(page.getByRole('group', { name: 'Show members', exact: true })).toBeVisible();
		await page.reload();
		await page.locator('html[data-hydrated="true"]').waitFor();
		await expect(page.getByRole('group', { name: 'Show members', exact: true })).toBeVisible();
		await page.evaluate(async () => {
			const path = '/e2e/fakes/org-api.ts';
			const { orgApi } = await import(path);
			const original = orgApi.view;
			orgApi.view = async (...args: unknown[]) => {
				orgApi.view = original;
				document.documentElement.dataset.forwardLoading = 'true';
				await new Promise<void>((resolve) =>
					window.addEventListener('release-forward-load', () => resolve(), { once: true })
				);
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
		await expect(
			page
				.getByRole('main')
				.getByRole('heading', { name: /^Projects/ })
				.first()
		).toBeVisible();
		await shot(page, 'shell-menu-back-during-forward');
	}
);

for (const width of [960, 1279]) {
	test(
		`SHELL-30 SET-200: at ${width}px with 200% text the shell reflows to the rail and settings keep adequate room`,
		{ tag: '@webkit' },
		async ({ page }) => {
			await createOrganization(page);
			await page.setViewportSize({ width, height: 900 });
			await visit(page, '/settings');
			await page.addStyleTag({ content: 'html { font-size: 200% !important; }' });
			await expect(page.locator('html')).toHaveCSS('font-size', '32px');
			const nav = page.getByRole('navigation', { name: 'Main', exact: true });
			// 4 rem icon rail (128 px at 32 px root) instead of 18 rem (576 px).
			expect((await nav.locator('..').boundingBox())?.width).toBeCloseTo(128, 0);
			const contentWidth = await page
				.locator('main h2')
				.first()
				.evaluate((el) => el.getBoundingClientRect().width);
			expect(contentWidth).toBeGreaterThanOrEqual(400);
			expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(
				width
			);
		}
	);
}
