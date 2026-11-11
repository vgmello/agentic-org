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

for (const [width, surface] of [[320, 'Conversation'], [320, 'Thread'], [640, 'Thread'], [960, 'Thread']] as const) {
	test(`CHAT-8/14 BUG-49: ${surface} keeps multiline keyboard focus visible at ${width}×256 with a larger font`, async ({ page }) => {
		await open(page, width, 256, 'c-release-v1');
		if (surface === 'Thread') {
			const opener = page.getByTestId('replies-link').first();
			await opener.scrollIntoViewIfNeeded();
			await opener.press('Enter');
		}
		const pane = page.getByRole(surface === 'Thread' ? 'complementary' : 'region', { name: surface, exact: true });
		const composer = pane.getByTestId('composer');
		await composer.fill(Array.from({ length: 8 }, (_, i) => `Draft line ${i + 1}`).join('\n'));
		await pane.getByRole('button', { name: 'Attach', exact: true }).press('Shift+Tab');
		await expect(composer).toBeFocused();
		const readPlacement = () => composer.evaluate((element) => {
			const box = element.getBoundingClientRect();
			const headerBottom = document.querySelector('[data-shell-header]')!.getBoundingClientRect().bottom;
			const x = box.left + box.width / 2;
			const visible = box.top >= headerBottom && box.bottom <= innerHeight
				&& [box.top + 1, box.bottom - 1].every((y) => element.contains(document.elementFromPoint(x, y)));
			return { visible, top: box.top, bottom: box.bottom, headerBottom, clipBottom: element.closest('[data-chat-panel]')?.getBoundingClientRect().bottom };
		});
		await expect.poll(async () => {
			const { visible, ...bounds } = await readPlacement();
			return visible ? 'visible' : JSON.stringify(bounds);
		}).toBe('visible');
		await shot(page, `chat-larger-default-font-${width}-${surface.toLowerCase()}-composer`);
	});
}

test('CHAT-5 BUG-49: Ask body stays 200px wide at 320×256 with a larger font', async ({ page }) => {
	await open(page, 320, 256, 'c-project-atlas');
	const question = page.getByTestId('message-list').getByText('Block bug reports when a project has no team?', { exact: true });
	await expect.poll(async () => (await question.boundingBox())?.width ?? 0).toBeGreaterThanOrEqual(200);
	await question.scrollIntoViewIfNeeded();
	await shot(page, 'chat-larger-default-font-320-ask');
});

test('CHAT-14 BUG-49: command suggestions fit the Conversation at 320px with a larger font', async ({ page }) => {
	await open(page, 320, 256, 'c-release-v1');
	const conversation = page.getByRole('region', { name: 'Conversation', exact: true });
	const composer = conversation.getByTestId('composer');
	await composer.press('/');
	const suggestions = conversation.getByRole('listbox', { name: 'Commands', exact: true });
	await expect(suggestions).toBeVisible();
	await expect.poll(() => conversation.evaluate((element) => element.scrollWidth - element.clientWidth)).toBe(0);
	const composerBounds = (await composer.boundingBox())!;
	const suggestionsBounds = (await suggestions.boundingBox())!;
	expect(suggestionsBounds.width).toBeLessThanOrEqual(composerBounds.width);
	await shot(page, 'chat-larger-default-font-320-command-suggestions');
});
