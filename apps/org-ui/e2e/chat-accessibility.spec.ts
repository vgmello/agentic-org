import { expect, test, type Page } from '@playwright/test';
import { visit } from './ready';
import { shot } from './shot';

async function open(page: Page) {
	await visit(page, '/create');
	await page.getByLabel('Organization name').fill('Acme');
	await page.getByRole('button', { name: 'Create organization' }).click();
	await expect(page).toHaveURL(/\/agents$/);
	await visit(page, '/chat');
	await expect(page.getByTestId('conversation-title')).toBeVisible();
}
const side = (page: Page) => page.getByRole('navigation', { name: 'Chat' });
const go = async (page: Page, name: string) => {
	if (!await side(page).isVisible()) await page.getByRole('button', { name: /^Conversations/ }).click();
	await side(page).getByTestId('conversation-link').filter({ hasText: name }).first().click();
};

for (const view of ['needs', 'mentions', 'threads', 'search'] as const) {
	test(`CHAT-1/13 BUG-50: ${view} is outside the message live log`, async ({ page }) => {
		await open(page);
		if (view === 'mentions' || view === 'threads') await side(page).getByTestId(`${view}-link`).click();
		if (view === 'search') {
			await go(page, 'general');
			await page.getByRole('searchbox', { name: 'Search chat' }).fill('create-org');
			await expect(page.getByTestId('search-count')).toContainText('result');
			await expect(page.getByTestId('message-list').getByTestId('message').first()).toBeVisible();
		}
		await shot(page, `chat-live-scope-${view}`);
		await expect(page.getByRole('log', { name: 'Messages', exact: true })).toHaveCount(0);
		await expect(page.getByRole('region', { name: 'Messages', exact: true })).toHaveCount(0);
		await expect(page.getByRole('region', { name: 'Conversation', exact: true })).toBeVisible();
		await expect(page.getByTestId('message-list')).toHaveAccessibleName('Message area');
		expect(await page.getByTestId('message-list').ariaSnapshot()).toContain('region "Message area"');
		await page.getByTestId('message-list').focus();
		await expect(page.getByTestId('message-list')).toBeFocused();
		expect(await page.getByTestId('message-list').evaluate((element) => element.closest('[role="log"], [aria-live]') === null)).toBe(true);
	});
}

test('CHAT-1/13: conversation landmarks have distinct names and the message area keeps programmatic focus', async ({ page }) => {
	await open(page);
	const conversation = page.getByRole('region', { name: 'Conversation', exact: true });
	const log = page.getByRole('log', { name: 'Messages', exact: true });
	const list = page.getByTestId('message-list');
	for (const name of ['general', 'Inês Duarte']) {
		await go(page, name);
		await expect(conversation).toBeVisible();
		await expect(log).toBeVisible();
		await expect(log).toHaveAttribute('aria-live', 'polite');
		await expect(log).toHaveAttribute('aria-relevant', 'additions');
		const tree = await conversation.ariaSnapshot();
		expect(tree).toContain('log "Messages"');
		expect(tree).not.toContain('region "Messages"');
		expect(tree).toContain('region "Message area"');
		await expect(page.getByRole('region', { name: 'Messages', exact: true })).toHaveCount(0);
		await expect(page.getByRole('region', { name: 'Message area', exact: true })).toHaveAttribute('data-testid', 'message-list');
		await expect(list).toHaveAccessibleName('Message area');
		await expect(list).toHaveAttribute('tabindex', '-1');
		await list.focus();
		await expect(list).toBeFocused();
		await expect(list).toHaveAttribute('data-focus-scope', '');
		await shot(page, `chat-distinct-landmarks-${name}`);
	}
});

test('CHAT-1/7 BUG-50: switching conversations replaces the log; new replies stay in the current polite log', async ({ page }) => {
	await open(page);
	await go(page, 'general');
	const log = page.getByRole('log', { name: 'Messages', exact: true });
	await expect(log).toContainText('Ready for review: create-org');
	const previous = await log.elementHandle();
	await go(page, 'release-v1');
	await expect(log).toContainText('Delivered: the offices screen is in v1.');
	expect(await previous!.evaluate((element) => element.isConnected)).toBe(false);
	await expect(log).toHaveAttribute('aria-live', 'polite');
	await shot(page, 'chat-live-scope-channel');
	await go(page, 'Inês Duarte');
	const composer = page.getByTestId('composer');
	await composer.fill('Could you check the live log?');
	await composer.press('Control+Enter');
	await expect(log).toContainText('Sample reply');
	await expect(composer).toBeFocused();
	await shot(page, 'chat-live-scope-new-reply');
});

for (const panel of ['thread', 'members'] as const) {
	test(`CHAT-1/8 BUG-49: ${panel} overlay excludes hidden controls and keeps keyboard focus visible at960`, async ({ page }) => {
		await page.setViewportSize({ width: 960, height: 640 });
		await open(page);
		await go(page, 'release-v1');
		const conversation = page.getByRole('region', { name: 'Conversation', exact: true, includeHidden: true });
		const opener = panel === 'thread' ? page.getByTestId('replies-link').first() : conversation.getByRole('button', { name: 'Members', exact: true });
		await opener.focus();
		await opener.press('Enter');
		const pane = page.getByTestId(`${panel}-panel`);
		await expect(pane).toBeVisible();
		await shot(page, `chat-${panel}-overlay-focus`);
		expect(await conversation.evaluate((element) => element.inert)).toBe(true);
		const panelBounds = await pane.boundingBox();
		const conversationBounds = await conversation.boundingBox();
		expect(panelBounds!.x).toBeCloseTo(conversationBounds!.x, 0);
		expect(panelBounds!.width).toBeCloseTo(conversationBounds!.width, 0);
		await expect(pane.getByRole('button', { name: `Close ${panel}` })).toBeFocused();
		let focusedControls = 0;
		for (let i = 0; i < 5; i++) {
			await page.keyboard.press('Tab');
			const focus = await page.evaluate(() => {
				const element = document.activeElement;
				// Native Tab can reach browser chrome, particularly in WebKit. Lost page focus is distinct from hidden DOM focus.
				if (!document.hasFocus() && element === document.body) return 'browser';
				if (!element || element === document.body) return 'lost';
				const r = element.getBoundingClientRect();
				const hit = document.elementFromPoint(r.x + r.width / 2, r.y + r.height / 2);
				return r.width > 0 && r.height > 0 && r.x >= 0 && r.right <= innerWidth && r.y >= 0 && r.bottom <= innerHeight && !!hit && (element === hit || element.contains(hit)) ? 'visible' : 'hidden';
			});
			expect(['visible', 'browser']).toContain(focus);
			if (focus === 'visible') focusedControls++;
		}
		expect(focusedControls).toBeGreaterThan(0);
		await page.keyboard.press('Escape');
		await expect(pane).toBeHidden();
		await expect(opener).toBeFocused();
		expect(await conversation.evaluate((element) => element.inert)).toBe(false);
		await shot(page, `chat-${panel}-overlay-closed-focus`);
	});
}

for (const [width, inline] of [[1055, false], [1056, true]] as const) {
	test(`CHAT-1 BUG-49: the conversation list is ${inline ? 'inline' : 'modal'} at content width ${width - 288}`, async ({ page }) => {
		await page.setViewportSize({ width, height: 720 });
		await open(page);
		const toggle = page.getByRole('button', { name: /^Conversations/ });
		if (inline) {
			await expect(side(page)).toBeVisible();
			await expect(toggle).toHaveCount(0);
		} else {
			await expect(toggle).toBeVisible();
			await expect(side(page)).toBeHidden();
		}
		await shot(page, `chat-sidebar-content-${width - 288}`);
	});
}

test('CHAT-1/3 BUG-49: the narrow conversation list is modal and restores selection or toggle focus', async ({ page }) => {
	await page.setViewportSize({ width: 960, height: 720 });
	await open(page);
	const toggle = page.getByRole('button', { name: 'Conversations, 2 need you', exact: true, includeHidden: true });
	await expect(toggle).toBeVisible();
	await toggle.focus(); await toggle.press('Enter');
	const dialog = page.getByRole('dialog', { name: 'Conversations', exact: true });
	await expect(dialog).toBeVisible();
	await expect(toggle).toHaveAttribute('aria-expanded', 'true');
	expect(await toggle.getAttribute('aria-controls')).toBe(await dialog.getAttribute('id'));
	await expect(side(page).getByTestId('needs-you-link')).toBeFocused();
	const conversation = page.getByRole('region', { name: 'Conversation', exact: true, includeHidden: true });
	expect(await conversation.evaluate((element) => element.inert)).toBe(true);
	await page.keyboard.press('Shift+Tab');
	await expect(dialog.getByRole('button', { name: 'Close', exact: true })).toBeFocused();
	await page.keyboard.press('Tab');
	await expect(side(page).getByTestId('needs-you-link')).toBeFocused();
	const ines = side(page).locator('[data-conversation="c-dm-ines"]');
	await ines.focus(); await ines.press('Enter');
	await expect(dialog).toBeHidden();
	await expect(page.getByTestId('conversation-title')).toHaveText('Inês Duarte');
	await expect(page.getByTestId('conversation-title')).toBeFocused();
	await toggle.click();
	await expect(ines).toBeFocused();
	expect(await ines.evaluate((element) => {
		const r = element.getBoundingClientRect(); const hit = document.elementFromPoint(r.x + r.width / 2, r.y + r.height / 2);
		return r.y >= 0 && r.bottom <= innerHeight && !!hit && (hit === element || element.contains(hit));
	})).toBe(true);
	await shot(page, 'chat-sidebar-selected-dm');
	await dialog.getByRole('button', { name: 'Close', exact: true }).click();
	await expect(dialog).toBeHidden(); await expect(toggle).toBeFocused();
	await toggle.press('Enter'); await expect(dialog).toBeVisible(); await page.keyboard.press('Escape');
	await expect(dialog).toBeHidden(); await expect(toggle).toBeFocused();
	await toggle.press('Enter'); await expect(dialog).toBeVisible();
	await page.locator('[data-slot="sheet-overlay"]').click({ position: { x: 950, y: 100 } });
	await expect(dialog).toBeHidden(); await expect(toggle).toBeFocused();
	await shot(page, 'chat-sidebar-closed-toggle');
	await toggle.press('Enter'); await expect(dialog).toBeVisible();
	await page.goBack();
	await expect(dialog).toBeHidden();
	await expect(page).toHaveURL(/c=c-dm-ines/);
	await expect(toggle).toBeFocused();
});

test('CHAT-2/3 BUG-49: narrow-sidebar dialogs return focus after creation or cancellation', async ({ page }) => {
	await page.setViewportSize({ width: 960, height: 720 });
	await open(page);
	const toggle = page.getByRole('button', { name: /^Conversations/ });
	await expect(toggle).toBeVisible();
	await toggle.click(); await side(page).getByRole('button', { name: 'Add channel' }).click();
	const channel = page.getByRole('dialog', { name: 'Add a channel' });
	await expect(channel.getByLabel('Name')).toBeFocused();
	await channel.getByLabel('Name').fill('accessibility-room');
	await channel.getByRole('button', { name: 'Add channel', exact: true }).click();
	await expect(channel).toBeHidden();
	await expect(page.getByRole('dialog', { name: 'Conversations', exact: true })).toBeHidden();
	await expect(page.getByTestId('conversation-title')).toHaveText('#accessibility-room');
	await expect(page.getByTestId('conversation-title')).toBeFocused();
	await toggle.click(); await side(page).getByRole('button', { name: 'New message' }).click();
	const message = page.getByRole('dialog', { name: 'New message', exact: true });
	await expect(message).toBeVisible(); await page.keyboard.press('Escape');
	await expect(message).toBeHidden(); await expect(toggle).toBeFocused();
	await shot(page, 'chat-sidebar-dialog-focus-return');
});


test('CHAT-1 BUG-49: resizing preserves visible focus as the conversation list changes modes', async ({ page }) => {
	await page.setViewportSize({ width: 1056, height: 720 });
	await open(page);
	const selected = side(page).getByTestId('needs-you-link');
	await selected.focus();
	await page.setViewportSize({ width: 960, height: 720 });
	const toggle = page.getByRole('button', { name: /^Conversations/ });
	await expect(toggle).toBeFocused();
	await toggle.press('Enter');
	const dialog = page.getByRole('dialog', { name: 'Conversations', exact: true });
	await expect(dialog).toBeVisible();
	await expect(selected).toBeFocused();
	await page.setViewportSize({ width: 1056, height: 720 });
	await expect(dialog).toBeHidden();
	await expect(toggle).toHaveCount(0);
	await expect(selected).toBeFocused();
	expect(await page.getByRole('region', { name: 'Conversation', exact: true }).evaluate((element) => element.inert)).toBe(false);
	await shot(page, 'chat-sidebar-resize-inline-focus');
});

test('CHAT-2/3 BUG-49: a sidebar dialog restores visible focus after a resize replaces its opener', async ({ page }) => {
	await page.setViewportSize({ width: 960, height: 720 });
	await open(page);
	await page.getByRole('button', { name: /^Conversations/ }).click();
	await side(page).getByRole('button', { name: 'Add channel' }).click();
	const dialog = page.getByRole('dialog', { name: 'Add a channel' });
	await expect(dialog.getByLabel('Name')).toBeFocused();
	await page.setViewportSize({ width: 1056, height: 720 });
	await page.keyboard.press('Escape');
	await expect(dialog).toBeHidden();
	await expect(side(page).getByTestId('needs-you-link')).toBeFocused();
	await shot(page, 'chat-sidebar-dialog-resize-focus');
});


test('CHAT-5/13 BUG-50: answering a search result keeps focus on the answered card outside the live log', async ({ page }) => {
	await open(page); await go(page, 'atlas');
	await page.getByRole('searchbox', { name: 'Search chat' }).fill('Block bug reports');
	const card = page.getByTestId('message-list').getByTestId('ask-card').filter({ hasText: 'Block bug reports' });
	await card.getByRole('button', { name: 'Yes, block', exact: true }).press('Enter');
	await expect(card.getByTestId('answered')).toContainText('Answered by');
	await expect(card).toBeFocused();
	await expect(page.getByRole('log', { name: 'Messages', exact: true })).toHaveCount(0);
	await shot(page, 'chat-search-answered-focus-outside-log');
});
