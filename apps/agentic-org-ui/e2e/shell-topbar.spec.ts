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

// ExecutiveCommandBar replaces the retired ShellTopbar & New menu: Cmd+K search/commands and Ask Org AI.
test('Command bar Cmd+K navigates cleanly and Ask Org AI answers live questions with focus return', async ({
	page
}) => {
	await enterMockedOrganization(page, 'Topbar reference');
	const cmdTrigger = page.getByRole('button', { name: /Search, run a command, or ask Org AI/ });
	await expect(cmdTrigger).toBeVisible();
	await page.keyboard.press('ControlOrMeta+k');
	const dialog = page.getByRole('dialog', { name: 'Command palette and Org AI' });
	await expect(dialog).toBeVisible();
	await expect(dialog.getByRole('combobox')).toBeFocused();
	await dialog.getByRole('combobox').fill('Go to Work');
	await page.keyboard.press('Enter');
	await expect(dialog).toBeHidden();
	await expect(page).toHaveURL(/\/work$/);
	await expect(page.locator('#shell-page-title')).toHaveText('Work');

	const askButton = page.getByRole('button', { name: 'Ask', exact: true });
	await askButton.click();
	await expect(dialog).toBeVisible();
	await expect(dialog.getByRole('tab', { name: 'Ask Org AI', exact: true })).toHaveAttribute(
		'aria-selected',
		'true'
	);
	await dialog.getByRole('button', { name: 'Team & office status' }).click();
	await expect(dialog).toContainText('agents active');
	await page.keyboard.press('Escape');
	await expect(dialog).toBeHidden();
	await expect(askButton).toBeFocused();

	// Hire page retains its heading, document title and non-dialog form directly from Members.
	await visit(page, '/teams/members');
	await page.getByRole('link', { name: 'Hire an agent', exact: true }).click();
	await expect(page).toHaveURL(/\/teams\/hire$/);
	const hireHeading = page.getByRole('heading', {
		level: 2,
		name: 'Hire an agent',
		exact: true
	});
	await expect(hireHeading).toBeVisible();
	await expect(hireHeading).toBeFocused();
	await expect(page).toHaveTitle('Hire an agent · Teams · Agentic Org');
	await expect(page.getByRole('dialog')).toHaveCount(0);
	await expect(page.getByRole('heading', { level: 3, name: 'Describe the hire' })).toBeVisible();
});

for (const width of [720, 960]) {
	test(
		`BUG-70 chrome: the Executive command bar keeps its height and controls at ${width}px`,
		{ tag: '@webkit' },
		async ({ page }) => {
			await page.setViewportSize({ width, height: 900 });
			await openOffice(page);
			const header = page.locator('header.exec-topbar');
			await expect(header).toBeVisible();
			await expect(page.locator('#shell-page-title')).toHaveText('Activity');
			await shot(page, `bug70-chrome-${width}`);
			const box = (await header.boundingBox())!;
			expect(box.height).toBe(68);
			expect(box.x).toBeGreaterThanOrEqual(264);
			expect(box.x + box.width).toBeLessThanOrEqual(width + 0.5);
			// The command bar holds the Cmd+K trigger and Ask button, with no inline nav links.
			await expect(
				header.getByRole('button', { name: /Search, run a command, or ask Org AI/ })
			).toBeVisible();
			await expect(header.getByRole('button', { name: 'Ask', exact: true })).toBeVisible();
			await expect(header.locator('a[href]')).toHaveCount(0);
		}
	);
}

test(
	'BUG-70 topbar: the current mobile Menu, area label and command bar remain accessible',
	{ tag: '@webkit' },
	async ({ page }) => {
		await page.setViewportSize({ width: 320, height: 600 });
		await openOffice(page);
		const menu = page.getByRole('button', { name: 'Menu', exact: true });
		await expect(menu).toBeVisible();
		await expect(menu.locator('..')).toContainText('Activity');
		await expect(page.locator('#shell-page-title')).toHaveText('Activity');
		const header = page.locator('header.exec-topbar');
		const bounds = (await header.boundingBox())!;
		expect(bounds.height).toBeGreaterThanOrEqual(60);
		expect(bounds.x + bounds.width).toBeLessThanOrEqual(320);
		await menu.press('Enter');
		await expect(page.getByRole('dialog', { name: 'Main navigation' })).toBeVisible();
		await page.keyboard.press('Escape');
		await expect(menu).toBeFocused();
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
					.getByRole('link', { name: 'Work', exact: true })
					.press('Enter');
			} else
				await page
					.getByRole('navigation', { name: 'Main', exact: true })
					.getByRole('link', { name: 'Work', exact: true })
					.press('Enter');
			await expect(page).toHaveURL(/\/work$/);
			const heading = page.getByRole('heading', { level: 1, name: 'Work', exact: true });
			await expect(heading).toHaveAttribute('id', 'shell-page-title');
			await shot(page, `bug70-heading-focus-${width}`);
			await expect(heading).toBeFocused();
		}
	);
}

// Explicit route/label pairs keep the oracle independent of the navigation implementation.
const shellRoutes = [
	['/home', 'Home', 'h1'],
	['/inbox', 'Inbox', 'h1'],
	['/projects', 'Projects', 'Projects'],
	['/projects/beacon', 'Projects', 'Beacon'],
	['/work', 'Work', 'h1'],
	['/chat', 'Chat', 'Needs you'],
	['/activity', 'Activity', 'h1'],
	['/teams', 'Teams', 'Teams'],
	['/teams/members', 'Teams', 'Members'],
	['/settings', 'Organization settings', 'General'],
	['/preferences', 'Your preferences', 'Your preferences']
] as const;

async function expectHeaderFits(
	page: Page,
	area: string,
	headingKind: string,
	width: number,
	textScale: number
) {
	const header = page.locator('header.exec-topbar');
	await expect(header).toBeVisible();
	await expect
		.poll(async () => (await header.boundingBox())?.x ?? -1)
		.toBeGreaterThanOrEqual(width >= 640 ? 264 - 0.5 : 0);
	const box = (await header.boundingBox())!;
	expect(box.height).toBeGreaterThanOrEqual(44);
	expect(box.height).toBeLessThanOrEqual(315);
	expect(box.x).toBeGreaterThanOrEqual(width >= 640 ? 264 - 0.5 : 0);
	expect(box.x + box.width).toBeLessThanOrEqual(width + 0.5);
	const heading =
		headingKind === 'h1'
			? page.locator('#shell-page-title')
			: area === 'Chat'
				? page
						.getByRole('main')
						.getByRole('heading', { level: 2, name: headingKind === 'Chat' ? /^#/ : headingKind })
				: page
						.getByRole('main')
						.getByRole('heading', { name: new RegExp(`^${headingKind}`) })
						.first();
	await expect(heading).toBeVisible();
	if (headingKind === 'h1') {
		await expect(heading).toHaveAttribute('tabindex', '-1');
	}
	// Measure painted visible text glyphs on the destination heading (excluding .sr-only spans)
	// so overflowing or clipped words fail at 200% text.
	const headingGlyphs = await heading.evaluate((element) => {
		const box = element.getBoundingClientRect();
		const walker = document.createTreeWalker(element, NodeFilter.SHOW_TEXT);
		const rects: { left: number; right: number; top: number; bottom: number; width: number }[] = [];
		while (walker.nextNode()) {
			const node = walker.currentNode;
			const parent = node.parentElement;
			if (!node.textContent?.trim() || parent?.closest('.sr-only')) continue;
			const range = document.createRange();
			range.selectNodeContents(node);
			for (const rect of range.getClientRects()) {
				if (rect.width > 0 && rect.height > 0) {
					rects.push({
						left: rect.left,
						right: rect.right,
						top: rect.top,
						bottom: rect.bottom,
						width: rect.width
					});
				}
			}
		}
		const mainRect = document.querySelector('main')!.getBoundingClientRect();
		return {
			count: rects.length,
			fitsBox: rects.every(
				(rect) =>
					rect.left >= box.left - 1 &&
					rect.right <= box.right + 1 &&
					rect.top >= box.top - 2 &&
					rect.bottom <= box.bottom + 2
			),
			fitsMainAndViewport: rects.every(
				(rect) =>
					rect.left >= Math.max(0, mainRect.left) - 1 &&
					rect.right <= Math.min(innerWidth, mainRect.right) + 1 &&
					rect.top >= Math.max(0, mainRect.top) - 2 &&
					rect.bottom <= Math.min(innerHeight, mainRect.bottom) + 2
			)
		};
	});
	expect(headingGlyphs.count).toBeGreaterThan(0);
	expect(headingGlyphs.fitsBox).toBe(true);
	expect(headingGlyphs.fitsMainAndViewport).toBe(true);

	const menu = page.getByRole('button', { name: 'Menu', exact: true });
	if (width < 640) {
		await expect(menu).toBeVisible();
		const button = (await menu.boundingBox())!;
		expect(button.x).toBeGreaterThanOrEqual(0);
		expect(button.x + button.width).toBeLessThanOrEqual(width);
		const mobileBar = menu.locator('..');
		await expect(mobileBar).toContainText(area);
		const mobileLabel = mobileBar.locator(':scope > span').last();
		const labelGlyphs = await mobileLabel.evaluate((element) => {
			const bar = element.parentElement!.getBoundingClientRect();
			const menuBox = element.parentElement!.querySelector('button')!.getBoundingClientRect();
			const range = document.createRange();
			range.selectNodeContents(element);
			const rects = Array.from(range.getClientRects(), (rect) => ({
				left: rect.left,
				right: rect.right,
				top: rect.top,
				bottom: rect.bottom,
				width: rect.width
			})).filter((rect) => rect.width > 0);
			return {
				count: rects.length,
				fitsBar: rects.every(
					(rect) =>
						rect.left >= menuBox.right - 1 &&
						rect.right <= Math.min(bar.right, innerWidth) + 1 &&
						rect.top >= bar.top - 1 &&
						rect.bottom <= bar.bottom + 1
				)
			};
		});
		expect(labelGlyphs.count).toBeGreaterThan(0);
		expect(labelGlyphs.fitsBar).toBe(true);
	} else {
		await expect(menu).not.toBeVisible();
		if (textScale === 1) expect(box.height).toBe(68);
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
				for (const [path, area, headingKind] of shellRoutes) {
					await visit(page, path);
					await page.addStyleTag({
						content: `html { font-size: ${textScale * 100}% !important; }`
					});
					await expectHeaderFits(page, area, headingKind, width, textScale);
					await shot(
						page,
						`bug70-${path.slice(1).replaceAll('/', '-')}-${width}-text${textScale * 100}`,
						{ context: page.locator('header.exec-topbar') }
					);
				}
			}
		);
	}
}

for (const [path, area, headingKind, state] of [
	['/teams/members?mock=empty', 'Teams', 'No members yet', 'empty'],
	['/teams/members?mock=fails', 'Teams', 'Members', 'failure'],
	['/chat?chat=offline&c=c-general', 'Chat', 'Chat', 'offline'],
	['/chat?chat=fail&c=c-general', 'Chat', 'Chat', 'failure']
] as const) {
	test(
		`BUG-70 preflight: ${path} keeps the header in the ${state} fixture`,
		{ tag: '@webkit' },
		async ({ page }) => {
			await page.setViewportSize({ width: 320, height: 900 });
			await openOffice(page);
			await visit(page, path);
			if (area === 'Teams' && state === 'empty')
				await expect(
					page.getByRole('heading', { name: 'No members yet', exact: true })
				).toBeVisible();
			else if (area === 'Teams')
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
			await expectHeaderFits(page, area, headingKind, 320, 1);
			await shot(page, `bug70-fixture-${area.toLowerCase()}-${state}`, {
				context: page.locator('header.exec-topbar')
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
		await expectHeaderFits(page, 'Activity', 'h1', 320, 2);
		await shot(page, 'bug70-fixture-stopped-text200', {
			context: page.locator('header.exec-topbar')
		});
	}
);

test(
	'BUG-70 chrome: breakpoint boundaries, fixed top frame and 264px sidebar preserve navigation geometry',
	{ tag: '@webkit' },
	async ({ page }) => {
		await openOffice(page);
		for (const width of [639, 640, 959, 960, 1023, 1024]) {
			await page.setViewportSize({ width, height: 600 });
			await expectHeaderFits(page, 'Activity', 'h1', width, 1);
			const header = page.locator('header.exec-topbar');
			const box = (await header.boundingBox())!;
			expect(box.y).toBeGreaterThanOrEqual(0);
			expect(box.y).toBeLessThanOrEqual(width < 640 ? 60 : 1);
			// Verify <main> genuinely scrolls via user mouse wheel while the command bar stays pinned outside <main>.
			await expect(page.locator('main header.exec-topbar')).toHaveCount(0);
			const main = page.getByRole('main');
			await main.evaluate((el) => {
				const spacer = document.createElement('div');
				spacer.dataset.topbarScrollSpacer = 'true';
				spacer.style.height = '1200px';
				el.append(spacer);
				el.scrollTop = 0;
			});
			try {
				const beforeTop = (await header.boundingBox())!.y;
				const mainBox = (await main.boundingBox())!;
				await page.mouse.move(
					mainBox.x + mainBox.width / 2,
					Math.min(mainBox.y + mainBox.height / 2, 580)
				);
				await page.mouse.wheel(0, 600);
				await expect.poll(() => main.evaluate((el) => el.scrollTop)).toBeGreaterThan(0);
				const afterTop = (await header.boundingBox())!.y;
				expect(Math.abs(afterTop - beforeTop)).toBeLessThanOrEqual(1);
			} finally {
				await main.evaluate((el) => {
					el.scrollTop = 0;
					el.querySelector('[data-topbar-scroll-spacer]')?.remove();
				});
			}
			if (width >= 640) {
				const aside = page.getByRole('complementary', { name: 'Sidebar', exact: true });
				await expect
					.poll(async () => Math.round((await aside.boundingBox())?.width ?? 0))
					.toBe(264);
			}
			await shot(page, `bug70-boundary-${width}`, { context: header });
		}
	}
);
