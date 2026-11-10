import { expect, test, type Page } from '@playwright/test';
import { visit } from './ready';
import { shot } from './shot';

async function open(page: Page, query = '') {
	await visit(page, '/create');
	await page.getByLabel('Organization name').fill('Acme');
	await page.getByRole('button', { name: 'Create organization' }).click();
	await expect(page).toHaveURL(/\/agents$/);
	await visit(page, `/chat${query}`);
	await expect(page.getByRole('navigation', { name: 'Chat' })).toBeVisible();
}
const side = (page: Page) => page.getByRole('navigation', { name: 'Chat' });
const go = (page: Page, name: string) => side(page).getByTestId('conversation-link').filter({ hasText: name }).first().click();

test('CHAT-8 BUG-56: Escape preserves a typed thread reply; an empty reply still closes the thread', async ({ page }) => {
	await open(page);
	await go(page, 'release-v1');
	await page.getByTestId('replies-link').first().click();
	const thread = page.getByTestId('thread-panel');
	const reply = thread.getByTestId('composer');
	await reply.fill('Half-written thread reply');
	await reply.press('Escape');
	await expect(thread).toBeVisible();
	await expect(reply).toHaveValue('Half-written thread reply');
	await expect(reply).toBeFocused();
	await expect(page).toHaveURL(/thread=/);
	await shot(page, 'chat-escape-retains-thread-reply');
	await reply.fill('');
	await reply.press('Escape');
	await expect(thread).toBeHidden();
	await expect(page).not.toHaveURL(/thread=/);
});

for (const action of ['reply', 'reject'] as const) {
	test(`CHAT-5/8 BUG-56: Escape preserves a typed ask ${action} inside a thread`, async ({ page }) => {
		await open(page);
		await go(page, 'atlas');
		const message = page.getByTestId('message').filter({ hasText: action === 'reply' ? 'Block bug reports' : 'Publish the 0.2 release notes' });
		await message.hover();
		await message.getByRole('button', { name: 'Reply in thread', exact: true }).click();
		const thread = page.getByTestId('thread-panel');
		const card = thread.getByTestId('ask-card');
		await card.getByRole('button', { name: action === 'reply' ? 'Reply…' : 'Reject…', exact: true }).click();
		const field = card.getByRole('textbox');
		await field.fill('Please keep this unfinished answer.');
		await field.press('Escape');
		await expect(thread).toBeVisible();
		await expect(field).toHaveValue('Please keep this unfinished answer.');
		await expect(field).toBeFocused();
		await shot(page, `chat-escape-retains-ask-${action}`);
	});
}

for (const word of ['Frobnicate', 'deploy2', 'ship-now', 'bgu']) {
	test(`CHAT-9 BUG-45d: /${word} stays editable until explicitly sent as a message`, async ({ page }) => {
		await open(page);
		await go(page, 'general');
		const composer = page.getByTestId('composer');
		const text = `/${word} now`;
		await composer.fill(text); await composer.press('Control+Enter');
		await expect(composer).toHaveValue(text);
		const advice = page.getByRole('alert').filter({ hasText: `No command /${word}.` });
		await expect(advice).toBeVisible();
		if (word === 'bgu') await expect(advice).toContainText('Did you mean /bug?');
		await expect(page.getByTestId('message-list')).not.toContainText(text);
		await shot(page, `chat-unknown-${word}`);
		await advice.getByRole('button', { name: 'Send as message', exact: true }).click();
		await expect(composer).toHaveValue('');
		await expect(advice).toHaveCount(0);
		const sent = page.getByTestId('message-list').getByTestId('message-body').filter({ hasText: text });
		await expect(sent).toHaveCount(1);
		await expect(sent).toHaveText(text);
		await expect(composer).toBeFocused();
		await shot(page, `chat-sent-literal-${word}`);
	});
}

test('CHAT-9 BUG-45d: known commands ignore case; leading space always sends ordinary text', async ({ page }) => {
	await open(page); await go(page, 'general');
	const composer = page.getByTestId('composer');
	await composer.fill('/BuG Case-insensitive title'); await composer.press('Control+Enter');
	const dialog = page.getByRole('dialog', { name: 'Report a bug', exact: true });
	await expect(dialog).toBeVisible();
	await expect(dialog.getByLabel('Title', { exact: true })).toHaveValue('Case-insensitive title');
	await page.keyboard.press('Escape'); await expect(dialog).toBeHidden();
	await composer.fill(' /bug This is a message'); await composer.press('Control+Enter');
	await expect(composer).toHaveValue('');
	await expect(dialog).toBeHidden();
	await expect(page.getByTestId('message-list').getByTestId('message-body').filter({ hasText: '/bug This is a message' })).toHaveText('/bug This is a message');
	await shot(page, 'chat-leading-space-literal');
});


test('CHAT-9 BUG-45d: correcting an unknown command clears the literal-send advice', async ({ page }) => {
	await open(page); await go(page, 'general');
	const composer = page.getByTestId('composer');
	await composer.fill('/unknown now'); await composer.press('Control+Enter');
	await expect(page.getByRole('button', { name: 'Send as message', exact: true })).toBeVisible();
	await composer.fill('/bug Corrected command');
	await expect(page.getByRole('button', { name: 'Send as message', exact: true })).toHaveCount(0);
	await expect(composer).not.toHaveAttribute('aria-invalid', 'true');
	await composer.press('Control+Enter');
	await expect(page.getByRole('dialog', { name: 'Report a bug', exact: true })).toBeVisible();
	await expect(page.getByTestId('message-list')).not.toContainText('/bug Corrected command');
});

test('CHAT-8/9 BUG-45d: sending an unknown thread command as text preserves its channel-copy option', async ({ page }) => {
	await open(page); await go(page, 'release-v1');
	await page.getByTestId('replies-link').first().click();
	const thread = page.getByTestId('thread-panel');
	await thread.getByTestId('composer').fill('/Ship-2 literal thread reply');
	await thread.getByRole('checkbox', { name: /Also send to/ }).check();
	await thread.getByTestId('composer').press('Control+Enter');
	await expect(thread.getByRole('alert')).toContainText('No command /Ship-2.');
	await thread.getByRole('button', { name: 'Send as message', exact: true }).click();
	await expect(thread.getByTestId('composer')).toHaveValue('');
	await expect(thread.getByTestId('message-body').filter({ hasText: '/Ship-2 literal thread reply' })).toHaveCount(1);
	await thread.getByRole('button', { name: 'Close thread' }).click();
	await expect(page.getByTestId('message-list').getByTestId('message-body').filter({ hasText: '/Ship-2 literal thread reply' })).toHaveCount(1);
	await shot(page, 'chat-literal-thread-channel-copy');
});


test('CHAT-1/2 BUG-57: Back dismisses Add channel before leaving its conversation', async ({ page }) => {
	await open(page); await go(page, 'general'); await expect(page).toHaveURL(/c=c-general/);
	await go(page, 'release-v1'); await expect(page).toHaveURL(/c=c-release-v1/);
	await side(page).getByRole('button', { name: 'Add channel', exact: true }).click();
	const dialog = page.getByRole('dialog', { name: 'Add a channel', exact: true });
	await dialog.getByLabel('Name').fill('half');
	await page.goBack();
	await expect(dialog).toBeHidden();
	await expect(page.getByTestId('conversation-title')).toHaveText('#release-v1');
	await expect(page).toHaveURL(/c=c-release-v1/);
	await shot(page, 'chat-back-dismisses-channel-dialog');
	await page.goBack();
	await expect(page.getByTestId('conversation-title')).toHaveText('#general');
});

test('CHAT-1/3 BUG-57: document titles name each channel, person and inbox view', async ({ page }) => {
	await open(page); await expect(page).toHaveTitle('Needs you · Chat · Agentic Org');
	await go(page, 'general'); await expect(page).toHaveTitle('#general · Chat · Agentic Org');
	await go(page, 'Inês Duarte'); await expect(page).toHaveTitle('Inês Duarte · Chat · Agentic Org');
	await side(page).getByTestId('mentions-link').click(); await expect(page).toHaveTitle('Mentions · Chat · Agentic Org');
	await side(page).getByTestId('threads-link').click(); await expect(page).toHaveTitle('Threads · Chat · Agentic Org');
	await shot(page, 'chat-place-title-threads');
});

test('CHAT-3 BUG-57: DM rows follow the 36px row size', async ({ page }) => {
	await open(page);
	const row = side(page).locator('[data-conversation="c-dm-sam"]');
	await row.scrollIntoViewIfNeeded();
	expect((await row.boundingBox())!.height).toBeCloseTo(36, 1);
	await shot(page, 'chat-dm-row-36px');
});

for (const target of ['message', 'ask'] as const) {
	test(`CHAT-5/14 BUG-57: clicking ${target} text does not focus its programmatic return target`, async ({ page }) => {
		await open(page);
		if (target === 'message') await go(page, 'general');
		const row = target === 'message'
			? page.getByTestId('message').filter({ hasText: 'Ready for review: create-org' })
			: page.getByTestId('ask-card').filter({ hasText: 'Block bug reports' });
		await row.locator(target === 'message' ? 'time' : 'p').first().click();
		await page.mouse.move(350, 30);
		await expect(row).not.toBeFocused();
		if (target === 'message') await expect(row.getByRole('toolbar', { name: 'Message actions' })).toHaveCSS('opacity', '0');
		await shot(page, `chat-pointer-${target}-no-stuck-focus`);
	});
}
