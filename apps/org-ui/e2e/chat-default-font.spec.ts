import { expect, test, type Page } from '@playwright/test';
import { visit } from './ready';
import { shot } from './shot';

async function open(page: Page, width: number, height: number, conversation = 'c-general') {
	await visit(page, '/create');
	await page.getByLabel('Organization name').fill('Acme');
	await page.getByRole('button', { name: 'Create organization' }).click();
	await expect(page).toHaveURL(/\/agents$/);
	await page.setViewportSize({ width, height });
	await visit(page, `/chat?c=${conversation}`);
	await expect(page.getByTestId('conversation-title')).toHaveText(`#${conversation.replace(/^c-(?:project-)?/, '')}`);
}

test.skip(({ browserName }) => browserName !== 'chromium', 'Chromium supplies a default-font launch option.');
test.use({ launchOptions: { args: ['--blink-settings=defaultFontSize=20'] } });

test('CHAT-8/11 BUG-49: panel positioning and focus agree at the 1280px breakpoint', async ({ page }) => {
	await open(page, 1440, 720, 'c-release-v1');
	expect(await page.evaluate(() => getComputedStyle(document.documentElement).fontSize)).toBe('20px');
	const opener = page.getByTestId('replies-link').first();
	await opener.press('Enter');
	const pane = page.getByTestId('thread-panel');
	const conversation = page.getByRole('region', { name: 'Conversation', exact: true, includeHidden: true });
	await expect(pane).toBeVisible();
	const wide = (await conversation.boundingBox())!;
	const widePane = (await pane.boundingBox())!;
	expect(widePane.x).toBeCloseTo(wide.x + wide.width, 0);
	expect(wide.y + wide.height).toBeCloseTo(720, 0);
	await expect(conversation).not.toHaveAttribute('inert', '');
	await page.setViewportSize({ width: 1200, height: 720 });
	await expect(conversation).toHaveAttribute('inert', '');
	await expect(pane.getByRole('button', { name: 'Close thread', exact: true })).toBeFocused();
	expect((await pane.boundingBox())!.x).toBeCloseTo((await conversation.boundingBox())!.x, 0);
	await shot(page, 'chat-larger-default-font-compact-thread');
});

test('CHAT-1 BUG-49: the inline Conversations list stays 256px with a larger default font', async ({ page }) => {
	await open(page, 1440, 720);
	expect(await page.evaluate(() => getComputedStyle(document.documentElement).fontSize)).toBe('20px');
	expect((await page.getByRole('navigation', { name: 'Chat', exact: true }).boundingBox())?.width).toBe(256);
	await shot(page, 'chat-larger-default-font-inline-list');
});
