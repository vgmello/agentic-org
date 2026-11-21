import { expect, test } from '@playwright/test';
import { shot } from './shot';
import {
	creationGeometry,
	largeTextFlow,
	kindGeometry,
	navigation,
	openChat,
	sidebarGeometry
} from './chat-followups';

for (const width of [320, 640, 960, 1280])
	for (const scale of [1, 2]) {
		test(`BUG-94 BUG-70: sidebar preserves whole words at real20px default ${width}/${scale * 100}`, async ({
			page
		}) => {
			await openChat(page, width, scale);
			await expect(page.locator('html')).toHaveCSS('font-size', `${20 * scale}px`);
			const nav = await navigation(page);
			await shot(page, `bug94-real20-${width}-${scale}`, { context: nav });
			await sidebarGeometry(nav);
		});
	}

for (const width of [320, 640, 960, 1280])
	for (const scale of [1, 2]) {
		if (width === 960 && scale === 2) continue; // Isolated BUG-119 below; all sidebar cases still run above.
		test(`BUG-95: real20px kind containment at ${width}/${scale * 100}`, async ({ page }) => {
			await openChat(page, width, scale, '/chat?c=c-general');
			const badge = page
				.getByTestId('message')
				.filter({ hasText: 'Ready for review: create-org' })
				.getByTitle('Announcement', { exact: true });
			await kindGeometry(badge);
			await shot(page, `bug95-real20-${width}-${scale}`, { context: badge.locator('..') });
		});
	}

test('BUG-119: rem-scaled shell gives kind metadata adequate room at real20px 960/200', async ({
	page
}) => {
	await openChat(page, 960, 2, '/chat?c=c-general');
	await expect(page.locator('html')).toHaveCSS('font-size', '40px');
	const badge = page
		.getByTestId('message')
		.filter({ hasText: 'Ready for review: create-org' })
		.getByTitle('Announcement', { exact: true });
	await expect(badge).toBeVisible();
	await shot(page, 'bug119-shell-real20-960-200', { context: badge.locator('..') });
	await kindGeometry(badge);
});

for (const width of [320, 640, 960, 1280])
	for (const scale of [1, 2]) {
		test(`BUG-134 BUG-135 BUG-136: layout paths at real20px default ${width}/${scale * 100}`, async ({
			page
		}) => {
			test.setTimeout(120_000);
			await largeTextFlow(page, width, scale);
			await expect(page.locator('html')).toHaveCSS('font-size', `${20 * scale}px`);
		});
	}

test('BUG-136: real20px default creation columns remain readable in BUG119 pane 960/200', async ({
	page
}) => {
	await openChat(page, 960, 2);
	await expect(page.locator('html')).toHaveCSS('font-size', '40px');
	const nav = await navigation(page);
	await creationGeometry(nav);
	await nav.getByRole('button', { name: 'New message', exact: true }).scrollIntoViewIfNeeded();
	await shot(page, 'bug136-real20-960-200', { context: nav });
});
