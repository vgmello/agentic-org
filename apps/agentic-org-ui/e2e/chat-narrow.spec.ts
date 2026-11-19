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

async function noSidewaysScroll(page: Page) {
	await expect.poll(() => page.getByRole('main').evaluate((main) => ({
		page: document.documentElement.scrollWidth - innerWidth,
		main: main.scrollWidth - main.clientWidth
	}))).toEqual({ page: 0, main: 0 });
}

const listToggle = (page: Page) => page.getByRole('button', { name: /^Conversations(?:,|$)/ });
const chatNav = (page: Page) => page.getByRole('navigation', { name: 'Chat', exact: true });
function visibleBelowHeader(element: Element) {
	const r = element.getBoundingClientRect();
	const hit = document.elementFromPoint(r.x + r.width / 2, r.y + r.height / 2);
	return r.y >= (document.querySelector('[data-shell-header]')?.getBoundingClientRect().bottom ?? 0) && r.bottom <= innerHeight && !!hit && element.contains(hit);
}

for (const [width, height, inline] of [[959, 720, true], [960, 720, false], [720, 450, false], [639, 720, false], [600, 720, false], [320, 256, false]] as const) {
	test(`CHAT-1 BUG-49: the real shell keeps Chat readable at ${width}×${height}`, async ({ page }) => {
		await open(page, width, height);
		await shot(page, `chat-narrow-${width}-initial`);
		const message = page.getByTestId('message-list').getByTestId('message-body').filter({ hasText: 'Ready for review: create-org' });
		await expect.poll(async () => (await message.boundingBox())?.width ?? 0).toBeGreaterThanOrEqual(200);
		await expect.poll(async () => (await page.getByTestId('message-list').boundingBox())?.height ?? 0).toBeGreaterThanOrEqual(48);
		await noSidewaysScroll(page);
		if (width === 320) {
			await message.scrollIntoViewIfNeeded();
			await expect(message).toBeInViewport({ ratio: 0.25 });
			await shot(page, 'chat-narrow-320-reading');
		}
		const conversation = page.getByRole('region', { name: 'Conversation', exact: true });
		if (width > 320) {
			const bounds = (await conversation.boundingBox())!;
			expect(bounds.y + bounds.height).toBeCloseTo(height, 0);
		}
		if (inline) {
			await expect(chatNav(page)).toBeVisible();
			expect((await chatNav(page).boundingBox())?.width).toBe(256);
			await expect(listToggle(page)).toHaveCount(0);
		} else {
			const toggle = listToggle(page);
			await expect(toggle).toHaveAccessibleName('Conversations, 2 need you');
			await toggle.press('Enter');
			const dialog = page.getByRole('dialog', { name: 'Conversations', exact: true });
			await expect(dialog).toBeVisible();
			await expect(chatNav(page).locator('[data-conversation="c-general"]')).toBeFocused();
			await shot(page, `chat-narrow-${width}-list`);
			const bounds = (await dialog.boundingBox())!;
			expect(bounds.x).toBeCloseTo(width >= 960 ? 288 : width >= 640 ? 64 : 0, 0);
			expect(bounds.width).toBeCloseTo(256, 0);
			expect(bounds.y).toBeGreaterThanOrEqual(0);
			expect(bounds.y + bounds.height).toBeLessThanOrEqual(height + 0.5);
			await chatNav(page).locator('[data-conversation="c-release-v1"]').press('Enter');
			await expect(dialog).toBeHidden();
			await expect(page.getByTestId('conversation-title')).toHaveText('#release-v1');
			await expect(page.getByTestId('conversation-title')).toBeFocused();
			if (width === 320) expect(await page.getByTestId('conversation-title').evaluate(visibleBelowHeader)).toBe(true);
		}
		await shot(page, `chat-narrow-${width}-conversation`);
	});
}

for (const panel of ['thread', 'members'] as const) {
	test(`CHAT-8/11 BUG-49: the ${panel} uses the full width at 320px`, async ({ page }) => {
		await open(page, 320, 256, 'c-release-v1');
		const opener = panel === 'thread' ? page.getByTestId('replies-link').first() : page.getByRole('button', { name: 'Members', exact: true });
		await opener.scrollIntoViewIfNeeded();
		await opener.press('Enter');
		const pane = page.getByTestId(`${panel}-panel`);
		await expect(pane).toBeVisible();
		const bounds = (await pane.boundingBox())!;
		expect(bounds.x).toBeCloseTo(0, 0);
		expect(bounds.width).toBeCloseTo(await page.getByRole('main').evaluate((main) => main.clientWidth), 0);
		const close = pane.getByRole('button', { name: `Close ${panel}`, exact: true });
		await expect(close).toBeFocused();
		expect(await close.evaluate(visibleBelowHeader)).toBe(true);
		if (panel === 'members') {
			const sam = pane.getByTestId('member-row').filter({ hasText: 'Sam Okafor' });
			await expect.poll(async () => (await sam.locator('p').first().boundingBox())?.width ?? 0).toBeGreaterThanOrEqual(200);
			await expect(sam.getByRole('combobox')).toBeVisible();
		} else {
			await expect.poll(async () => (await pane.getByRole('log').boundingBox())?.height ?? 0).toBeGreaterThanOrEqual(48);
			await expect.poll(async () => (await pane.getByTestId('message-body').first().boundingBox())?.width ?? 0).toBeGreaterThanOrEqual(200);
		}
		await noSidewaysScroll(page);
		await shot(page, `chat-narrow-320-${panel}`);
		await close.click();
		await expect(pane).toBeHidden();
		await expect(opener).toBeFocused();
		expect(await opener.evaluate(visibleBelowHeader)).toBe(true);
	});
}

test('CHAT-2/3 BUG-49: Chat dialogs are full width and their buttons are reachable at 320px', async ({ page }) => {
	await open(page, 320, 256);
	for (const [action, title] of [['Add channel', 'Add a channel'], ['New message', 'New message'], ['Command', 'Report a bug']] as const) {
		if (action === 'Command') {
			const composer = page.getByTestId('composer');
			await composer.fill('/bug Narrow dialog'); await composer.press('Control+Enter');
		} else {
			await listToggle(page).click();
			await chatNav(page).getByRole('button', { name: action, exact: true }).click();
		}
		const dialog = page.getByRole('dialog', { name: title, exact: true });
		await expect(dialog).toBeVisible();
		await expect.poll(async () => (await dialog.boundingBox())?.width ?? 0).toBeCloseTo(320, 0);
		const cancel = dialog.getByRole('button', { name: 'Cancel', exact: true });
		await cancel.scrollIntoViewIfNeeded();
		await expect(cancel).toBeInViewport();
		await shot(page, `chat-narrow-320-${action.toLowerCase().replaceAll(' ', '-')}`);
		await cancel.click();
		await expect(dialog).toBeHidden();
		if (action !== 'Command') await expect(listToggle(page)).toBeFocused();
	}
});


test('CHAT-5 BUG-49: the question text in a Chat message stays readable at 320px', async ({ page }) => {
	await open(page, 320, 256, 'c-project-atlas');
	const question = page.getByTestId('message-list').getByText('Block bug reports when a project has no team?', { exact: true });
	await expect.poll(async () => (await question.boundingBox())?.width ?? 0).toBeGreaterThanOrEqual(200);
	await noSidewaysScroll(page);
	await shot(page, 'chat-narrow-320-ask');
});

for (const panel of ['thread', 'members'] as const) {
	test(`CHAT-8/11 BUG-49: tabbing through the ${panel} stays visible after scrolling at 320px`, async ({ page }) => {
		await open(page, 320, 256, 'c-release-v1');
		const opener = panel === 'thread' ? page.getByTestId('replies-link').first() : page.getByRole('button', { name: 'Members', exact: true });
		await opener.scrollIntoViewIfNeeded();
		await opener.press('Enter');
		const pane = page.getByTestId(`${panel}-panel`);
		await expect(pane.getByRole('button', { name: `Close ${panel}`, exact: true })).toBeFocused();
		const focused = pane.locator(':focus');
		const composer = pane.getByTestId('composer');
		let visited = 0;
		async function checkFocus() {
			if (!await focused.count()) return;
			visited++;
			await expect.poll(() => focused.evaluate((element) => {
				const r = element.getBoundingClientRect();
				const hit = document.elementFromPoint(r.x + r.width / 2, r.y + r.height / 2);
				return { visible: r.y >= (document.querySelector('[data-shell-header]')?.getBoundingClientRect().bottom ?? 0) && r.bottom <= innerHeight && !!hit && element.contains(hit), top: r.y, bottom: r.bottom, control: element.getAttribute('aria-label') ?? element.textContent?.trim() };
			})).toMatchObject({ visible: true });
		}
		if (panel === 'thread') {
			for (let i = 0; i < 40 && !await composer.evaluate((element) => element === document.activeElement); i++) await page.keyboard.press('Tab');
			await expect(composer).toBeFocused();
			await checkFocus();
		} else {
			for (let i = 0; i < 7; i++) { await page.keyboard.press('Tab'); await checkFocus(); }
		}
		for (let i = 0; i < 12; i++) { await page.keyboard.press('Shift+Tab'); await checkFocus(); }
		expect(visited).toBeGreaterThan(0);
		await shot(page, `chat-narrow-320-${panel}-tab-after-scroll`);
	});
}

async function openShortThread(page: Page, width = 320) {
	await open(page, width, 256, 'c-release-v1');
	const opener = page.getByTestId('replies-link').first();
	await opener.scrollIntoViewIfNeeded(); await opener.press('Enter');
	const thread = page.getByTestId('thread-panel');
	await expect(thread.getByRole('button', { name: 'Close thread', exact: true })).toBeFocused();
	return thread;
}
const multiline = Array.from({ length: 8 }, (_, i) => `Draft line ${i + 1}`).join('\n');
const fixtureFile = (name: string) => ({ name, mimeType: 'text/plain', buffer: Buffer.from('test attachment') });

test('CHAT-8/14 BUG-49: a multiline thread draft stays visible when focus returns at 320px', async ({ page }) => {
	const thread = await openShortThread(page);
	const composer = thread.getByTestId('composer');
	await composer.fill(multiline);
	const attach = thread.getByRole('button', { name: 'Attach', exact: true });
	await attach.focus(); await attach.press('Shift+Tab');
	await expect(composer).toBeFocused();
	await expect.poll(() => composer.evaluate(visibleBelowHeader)).toBe(true);
	await expect(composer).toHaveValue(multiline);
	await shot(page, 'chat-narrow-320-multiline-focus');
});

test('CHAT-8/14 BUG-49: a multiline draft with ten attachments leaves a readable thread log at 320px', async ({ page }) => {
	const thread = await openShortThread(page);
	await thread.getByTestId('composer').fill(multiline);
	const files = Array.from({ length: 10 }, (_, i) => fixtureFile(`thread-draft-attachment-${i + 1}.txt`));
	await thread.getByLabel('Attach files').setInputFiles(files);
	const attached = thread.getByRole('list', { name: 'Attached', exact: true });
	await expect(attached.getByRole('listitem')).toHaveCount(10);
	await expect.poll(async () => (await thread.getByRole('log').boundingBox())?.height ?? 0).toBeGreaterThanOrEqual(48);
	await attached.getByRole('button', { name: 'Remove thread-draft-attachment-10.txt', exact: true }).click();
	await expect(attached.getByRole('listitem')).toHaveCount(9);
	await noSidewaysScroll(page);
	await shot(page, 'chat-narrow-320-attached-draft');
});

test('CHAT-14 BUG-49: a long attachment name fits without sideways scrolling at 320px', async ({ page }) => {
	const thread = await openShortThread(page);
	const name = `${'x'.repeat(220)}.txt`;
	await thread.getByLabel('Attach files').setInputFiles(fixtureFile(name));
	const attached = thread.getByRole('list', { name: 'Attached', exact: true });
	await expect(attached).toContainText(name);
	await noSidewaysScroll(page);
	await expect.poll(() => thread.evaluate((element) => element.scrollWidth - element.clientWidth)).toBe(0);
	await expect.poll(() => attached.evaluate((element) => element.scrollWidth - element.clientWidth)).toBe(0);
	await attached.getByRole('button', { name: `Remove ${name}`, exact: true }).scrollIntoViewIfNeeded();
	await shot(page, 'chat-narrow-320-long-attachment');
});

test('CHAT-1/14 BUG-49: reverse Tab keeps the Conversation composer and message actions visible at 320×256', async ({ page, browserName }) => {
	await open(page, 320, 256, 'c-release-v1');
	const conversation = page.getByRole('region', { name: 'Conversation', exact: true });
	const composer = conversation.getByTestId('composer');
	await composer.fill(multiline);
	const attach = conversation.getByRole('button', { name: 'Attach', exact: true });
	await attach.focus(); await attach.press('Shift+Tab');
	await expect(composer).toBeFocused();
	await expect.poll(() => composer.evaluate(visibleBelowHeader)).toBe(true);
	// WebKit on macOS uses Option+Tab to include buttons in keyboard traversal.
	await composer.press(browserName === 'webkit' && process.platform === 'darwin' ? 'Alt+Shift+Tab' : 'Shift+Tab');
	const messageAction = conversation.getByTestId('message-list').locator('button:focus');
	await expect(messageAction).toBeFocused();
	await expect.poll(() => messageAction.evaluate(visibleBelowHeader)).toBe(true);
	await shot(page, 'chat-narrow-320-conversation-reverse-tab');
});

for (const width of [640, 960]) {
	test(`CHAT-8/14 BUG-49: the thread log and composer remain usable at ${width}×256`, async ({ page }) => {
		const thread = await openShortThread(page, width);
		const composer = thread.getByTestId('composer');
		await composer.fill(multiline);
		const attach = thread.getByRole('button', { name: 'Attach', exact: true });
		await attach.focus(); await attach.press('Shift+Tab');
		await expect(composer).toBeFocused();
		await expect.poll(() => composer.evaluate(visibleBelowHeader)).toBe(true);
		await expect.poll(async () => (await thread.getByRole('log').boundingBox())?.height ?? 0).toBeGreaterThanOrEqual(48);
		await noSidewaysScroll(page);
		await shot(page, `chat-narrow-${width}-short-thread`);
	});
}

test('CHAT-14 BUG-49: a sent long attachment stays within the thread log at 320px', async ({ page }) => {
	const thread = await openShortThread(page);
	const name = `${'x'.repeat(220)}.txt`;
	await thread.getByTestId('composer').fill('A reply with an attachment');
	await thread.getByLabel('Attach files').setInputFiles(fixtureFile(name));
	await thread.getByRole('button', { name: /^Send/ }).click();
	const log = thread.getByRole('log', { name: 'Thread messages', exact: true });
	const sent = log.getByRole('list', { name: 'Attachments', exact: true }).getByText(name, { exact: false });
	await expect(sent).toBeVisible();
	await expect.poll(() => log.evaluate((element) => element.scrollWidth - element.clientWidth)).toBe(0);
	await sent.scrollIntoViewIfNeeded();
	await shot(page, 'chat-narrow-320-sent-long-attachment');
});
