import { expect, test, type Page } from '@playwright/test';
import { visit } from './ready';
import { shot } from './shot';
import { enterMockedOrganization } from './mock-organization';

async function openOffice(page: Page, createPath = '/create') {
	await enterMockedOrganization(page, 'Topbar reference', createPath);
	await expect(page).toHaveURL(/\/teams\/members$/);
	await visit(page, '/activity');
	await expect(
		page.getByRole('heading', { level: 1, name: 'Activity', exact: true })
	).toBeVisible();
}

// app-shell-ux §0.2 and our-org-in-the-app §7: New › Hire an agent opens the Hire page; no sheet opens.
test('New › Hire an agent opens the Hire page with its heading, title and focus', async ({
	page
}) => {
	await enterMockedOrganization(page, 'Topbar reference');
	await page.getByRole('button', { name: 'New', exact: true }).click();
	await page.getByRole('menuitem', { name: 'Hire an agent', exact: true }).click();
	await expect(page).toHaveURL(/\/teams\/hire$/);
	await expect(
		page.getByRole('heading', { level: 2, name: 'Hire an agent', exact: true })
	).toBeVisible();
	await expect(page).toHaveTitle('Hire an agent · Teams · Agentic Org');
	await expect(page.getByRole('dialog')).toHaveCount(0);
	await expect(page.getByRole('heading', { level: 3, name: 'Describe the hire' })).toBeVisible();
	await expect(page.locator('main h1')).toBeFocused();
});

for (const width of [720, 960]) {
	test(
		`BUG-70 chrome: the heading follows prototype spacing at ${width}px`,
		{ tag: '@webkit' },
		async ({ page }) => {
			await page.setViewportSize({ width, height: 900 });
			await openOffice(page);
			const header = page.locator('[data-shell-header]');
			const heading = header.getByRole('heading', { name: 'Activity', exact: true });
			await expect(heading).toBeVisible();
			await shot(page, `bug70-chrome-${width}`);
			const box = await header.boundingBox(),
				title = await heading.boundingBox();
			expect(box?.height).toBe(64);
			expect(title!.x - box!.x).toBe(24);
			// app-shell-ux §0.2: the bar's only field is Search, and it holds no links.
			await expect(header.locator('input')).toHaveCount(1);
			await expect(header.locator('input[type="search"]')).toBeVisible();
			await expect(header.locator('a[href]')).toHaveCount(0);
		}
	);
}

test(
	'BUG-70 topbar: the current mobile Menu and page heading remain accessible',
	{ tag: '@webkit' },
	async ({ page }) => {
		await page.setViewportSize({ width: 320, height: 600 });
		await openOffice(page);
		const header = page.locator('[data-shell-header]');
		await expect(header.getByRole('button', { name: 'Menu', exact: true })).toBeVisible();
		await expect(header.getByRole('heading', { name: 'Activity', exact: true })).toBeVisible();
		const bounds = await header.boundingBox();
		expect(bounds?.height).toBe(56);
		expect(bounds!.x + bounds!.width).toBeLessThanOrEqual(320);
		await header.getByRole('button', { name: 'Menu', exact: true }).press('Enter');
		await expect(page.getByRole('dialog', { name: 'Main navigation' })).toBeVisible();
		await page.keyboard.press('Escape');
		await expect(header.getByRole('button', { name: 'Menu', exact: true })).toBeFocused();
		await shot(page, 'bug70-mobile-header');
	}
);

for (const width of [320, 720]) {
	test(
		`BUG-70 chrome: route navigation retains the heading id and focusability at ${width}px`,
		{ tag: '@webkit' },
		async ({ page }) => {
			await page.setViewportSize({ width, height: 600 });
			await openOffice(page);
			if (width < 640) {
				await page.getByRole('button', { name: 'Menu', exact: true }).press('Enter');
				await page
					.getByRole('dialog', { name: 'Main navigation' })
					.getByRole('link', { name: 'Teams', exact: true })
					.press('Enter');
			} else
				await page
					.getByRole('navigation', { name: 'Main', exact: true })
					.getByRole('link', { name: 'Teams', exact: true })
					.press('Enter');
			await expect(page).toHaveURL(/\/teams$/);
			const heading = page.getByRole('heading', { level: 1, name: 'Teams', exact: true });
			await expect(heading).toHaveAttribute('id', 'shell-page-title');
			await shot(page, `bug70-heading-focus-${width}`);
			if (width >= 640) await heading.focus(); // Automatic desktop landing is BUG-74, a separate slice.
			await expect(heading).toBeFocused();
		}
	);
}

// Explicit route/label pairs keep the oracle independent of the navigation implementation.
const shellRoutes = [
	['/home', 'Home'],
	['/inbox', 'Inbox'],
	['/projects', 'Projects'],
	['/projects/beacon', 'Projects'],
	['/work', 'Work'],
	['/chat', 'Chat'],
	['/activity', 'Activity'],
	['/teams', 'Teams'],
	['/teams/members', 'Teams'],
	['/settings', 'Organization settings'],
	['/preferences', 'Your preferences']
] as const;

async function expectHeaderFits(page: Page, name: string, width: number, textScale: number) {
	const header = page.locator('[data-shell-header]');
	const heading = header.getByRole('heading', { level: 1, name, exact: true });
	await expect(heading).toBeVisible();
	await expect(heading).toHaveAttribute('id', 'shell-page-title');
	await expect(heading).toHaveAttribute('tabindex', '-1');
	const box = (await header.boundingBox())!;
	// Below 640 px the bar is one 56 px row at normal text; at larger text Search and New may take a second row so the
	// title keeps its room (SHELL-6, app-shell-ux §6a "yield to the narrower layout").
	// A long area name ("Organization settings") wraps rather than being cut off, so only short names keep one row.
	if (width >= 640 || (textScale === 1 && name.length <= 12))
		expect(box.height).toBe((width < 640 ? 56 : 64) * textScale);
	else {
		expect(box.height).toBeGreaterThanOrEqual(56 * textScale);
		// Never more than about a third of a 900 px viewport, even for the longest area name at 200%.
		expect(box.height).toBeLessThanOrEqual(315);
	}
	expect(box.x).toBeGreaterThanOrEqual(0);
	expect(box.x + box.width).toBeLessThanOrEqual(width);
	// Measure painted text, not just the flex item's box, so overflowing words fail.
	const lines = await heading.evaluate((element) => {
		const range = document.createRange();
		range.selectNodeContents(element);
		return Array.from(range.getClientRects(), (rect) => ({
			left: rect.left,
			right: rect.right,
			top: rect.top,
			bottom: rect.bottom
		}));
	});
	expect(lines.length).toBeGreaterThan(0);
	for (const line of lines) {
		expect(line.left).toBeGreaterThanOrEqual(box.x);
		expect(line.right).toBeLessThanOrEqual(box.x + box.width);
		expect(line.top).toBeGreaterThanOrEqual(box.y);
		expect(line.bottom).toBeLessThanOrEqual(box.y + box.height);
	}
	const menu = header.getByRole('button', { name: 'Menu', exact: true });
	if (width < 640) {
		await expect(menu).toBeVisible();
		const button = (await menu.boundingBox())!;
		expect(button.x).toBeGreaterThanOrEqual(box.x);
		expect(button.x + button.width).toBeLessThanOrEqual(box.x + box.width);
	} else {
		await expect(menu).not.toBeVisible();
		const title = (await heading.boundingBox())!;
		expect(title.x - box.x).toBe((width < 1024 ? 24 : 32) * textScale);
	}
}

for (const width of [320, 640, 960, 1280]) {
	for (const textScale of [1, 2]) {
		test(
			`BUG-70 preflight: all shell render paths at ${width}px and ${textScale * 100}% text`,
			{ tag: '@webkit' },
			async ({ page }) => {
				test.setTimeout(120_000);
				await page.setViewportSize({ width, height: 900 });
				await openOffice(page);
				for (const [path, heading] of shellRoutes) {
					await visit(page, path);
					await page.addStyleTag({
						content: `html { font-size: ${textScale * 100}% !important; }`
					});
					await expectHeaderFits(page, heading, width, textScale);
					await shot(
						page,
						`bug70-${path.slice(1).replaceAll('/', '-')}-${width}-text${textScale * 100}`,
						{ context: page.locator('[data-shell-header]') }
					);
				}
			}
		);
	}
}

for (const [path, heading, state] of [
	['/teams/members?mock=empty', 'Teams', 'empty'],
	['/teams/members?mock=fails', 'Teams', 'failure'],
	['/chat?chat=offline&c=c-general', 'Chat', 'offline'],
	['/chat?chat=fail&c=c-general', 'Chat', 'failure']
] as const) {
	test(
		`BUG-70 preflight: ${path} keeps the header in the ${state} fixture`,
		{ tag: '@webkit' },
		async ({ page }) => {
			await page.setViewportSize({ width: 320, height: 900 });
			await openOffice(page);
			await visit(page, path);
			if (heading === 'Teams' && state === 'empty')
				await expect(
					page.getByRole('heading', { name: 'No members yet', exact: true })
				).toBeVisible();
			else if (heading === 'Teams')
				await expect(page.getByText("Couldn't load the members", { exact: true })).toBeVisible();
			else if (state === 'offline') await expect(page.getByTestId('office-down')).toBeVisible();
			else {
				const composer = page.getByTestId('composer').first();
				await composer.fill('Header failure fixture');
				await composer.press('Control+Enter');
				await expect(page.getByTestId('not-delivered')).toContainText(
					"Not delivered: The office didn't answer."
				);
			}
			await expectHeaderFits(page, heading, 320, 1);
			await shot(page, `bug70-fixture-${heading.toLowerCase()}-${state}`, {
				context: page.locator('[data-shell-header]')
			});
		}
	);
}

test(
	'BUG-70 preflight: stopped office fixture keeps chrome at 200% text',
	{ tag: '@webkit' },
	async ({ page }) => {
		await page.setViewportSize({ width: 320, height: 900 });
		await openOffice(page, '/create?office=crashed');
		await page.getByRole('button', { name: 'Menu', exact: true }).press('Enter');
		await expect(
			page
				.getByRole('dialog', { name: 'Main navigation' })
				.getByRole('link', { name: /^HQ office · / })
		).toHaveAccessibleName('HQ office · Stopped');
		await page.keyboard.press('Escape');
		await page.addStyleTag({ content: 'html { font-size: 200% !important; }' });
		await expectHeaderFits(page, 'Activity', 320, 2);
		await shot(page, 'bug70-fixture-stopped-text200', {
			context: page.locator('[data-shell-header]')
		});
	}
);

test(
	'BUG-70 chrome: breakpoint boundaries, gap and sticky frame preserve navigation geometry',
	{ tag: '@webkit' },
	async ({ page }) => {
		await openOffice(page);
		for (const width of [639, 640, 959, 960, 1023, 1024]) {
			await page.setViewportSize({ width, height: 600 });
			await expectHeaderFits(page, 'Activity', width, 1);
			const header = page.locator('[data-shell-header]');
			await expect(header).toHaveCSS('column-gap', width < 640 ? '12px' : '24px');
			await expect(header).toHaveCSS(
				'padding-left',
				`${width < 640 ? 16 : width < 1024 ? 24 : 32}px`
			);
			await expect(header).toHaveCSS('position', 'sticky');
			await expect(header).toHaveCSS('top', '0px');
			if (width >= 640) {
				const nav = page.getByRole('navigation', { name: 'Main', exact: true });
				expect((await nav.locator('..').boundingBox())!.width).toBe(width < 960 ? 64 : 288);
			}
			await shot(page, `bug70-boundary-${width}`, { context: header });
		}
	}
);
