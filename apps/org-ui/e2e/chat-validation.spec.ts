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


for (const place of ['conversation', 'thread'] as const) {
	test(`CHAT-8/11 BUG-58: a stopped office disables reaction, pin and mute in the ${place}`, async ({ page }) => {
		await open(page, '?chat=offline'); await go(page, 'general');
		let row = page.getByTestId('message').filter({ hasText: 'Ready for review: create-org' });
		if (place === 'thread') {
			await row.getByTestId('replies-link').click();
			row = page.getByTestId('thread-panel').getByTestId('message').filter({ hasText: 'Ready for review: create-org' });
		}
		await row.hover();
		await expect(row.getByRole('button', { name: 'React Seen', exact: true })).toBeDisabled();
		await expect(row.getByRole('button', { name: 'Seen, 1', exact: true })).toBeDisabled();
		await row.getByRole('button', { name: 'More actions', exact: true }).click();
		await expect(page.getByRole('menuitem', { name: 'Pin to the conversation', exact: true })).toBeDisabled();
		await expect(page.getByRole('menuitem', { name: 'Mute Sam Okafor here', exact: true })).toBeDisabled();
		await expect(page.getByRole('menuitem', { name: 'Daily digest from Sam Okafor', exact: true })).toBeDisabled();
		await page.getByRole('menuitem', { name: 'Daily digest from Sam Okafor', exact: true }).scrollIntoViewIfNeeded();
		await shot(page, `chat-stopped-${place}-actions`);
	});
}

test('CHAT-11 BUG-58: stopped-office member mute controls are disabled and guard programmatic events', async ({ page }) => {
	await open(page, '?chat=offline'); await go(page, 'general');
	await page.evaluate(async () => {
		const path = '/src/lib/modules/messaging/legacy-api.ts';
		const { chatApi } = await import(path);
		const command = chatApi.command.bind(chatApi);
		(window as unknown as { chatMutations: string[] }).chatMutations = [];
		chatApi.command = async (id: string, call: { method: string }) => {
			if (['chat.react', 'chat.pin', 'chat.mute'].includes(call.method)) (window as unknown as { chatMutations: string[] }).chatMutations.push(call.method);
			return command(id, call);
		};
	});
	const row = page.getByTestId('message').filter({ hasText: 'Ready for review: create-org' });
	await row.getByRole('button', { name: 'React Seen', exact: true }).dispatchEvent('click');
	await page.getByRole('button', { name: 'Members', exact: true }).click();
	const select = page.getByTestId('members-panel').getByLabel('How you hear from Sam Okafor here');
	await expect(select).toBeDisabled();
	await select.evaluate((element) => { (element as HTMLSelectElement).value = 'muted'; element.dispatchEvent(new Event('change', { bubbles: true })); });
	expect(await page.evaluate(() => (window as unknown as { chatMutations: string[] }).chatMutations)).toEqual([]);
	await expect(page.getByText("The office didn't answer.", { exact: true })).toHaveCount(0);
	await shot(page, 'chat-stopped-members-mute-disabled');
});

test('CHAT-11 BUG-58: a running office still accepts reactions, pins and mute changes', async ({ page }) => {
	await open(page); await go(page, 'general');
	const row = page.getByTestId('message').filter({ hasText: 'Ready for review: create-org' });
	await row.hover(); await row.getByRole('button', { name: 'React Seen', exact: true }).click();
	await expect(row.getByRole('button', { name: 'Seen, 2', exact: true })).toHaveAttribute('aria-pressed', 'true');
	await row.getByRole('button', { name: 'More actions', exact: true }).click();
	await page.getByRole('menuitem', { name: 'Pin to the conversation', exact: true }).click();
	await expect(row).toContainText('Pinned');
	await page.getByRole('button', { name: 'Members', exact: true }).click();
	const select = page.getByTestId('members-panel').getByLabel('How you hear from Sam Okafor here');
	await expect(select).toBeEnabled();
	await select.selectOption('digest'); await expect(select).toHaveValue('digest');
	await shot(page, 'chat-running-actions-still-enabled');
});


test('CHAT-5/14 BUG-57: an intentional answer focus target stops accepting pointer focus after blur', async ({ page }) => {
	await open(page); await go(page, 'atlas');
	const card = page.getByTestId('ask-card').filter({ hasText: 'Block bug reports' });
	await card.getByRole('button', { name: 'Reply…', exact: true }).click();
	await card.getByRole('textbox').fill('Please wait for the audit.');
	const send = card.getByRole('button', { name: 'Send reply', exact: true });
	await send.focus(); await send.press('Enter');
	await expect(card).toBeFocused();
	await page.getByRole('searchbox', { name: 'Search chat' }).focus();
	await expect(card).not.toHaveAttribute('tabindex');
	await card.locator('p').first().click(); await page.mouse.move(350, 30);
	await expect(card).not.toBeFocused();
	await shot(page, 'chat-answer-focus-cleans-up-after-blur');
});

for (const kind of ['channel', 'dm'] as const) {
	test(`CHAT-2/3 BUG-57: Back preserves a pending ${kind} submission and its conversation`, async ({ page }) => {
		await open(page); await go(page, 'general'); await expect(page).toHaveURL(/c=c-general/);
		await go(page, 'release-v1'); await expect(page).toHaveURL(/c=c-release-v1/);
		await page.evaluate(async (method) => {
			const path = '/src/lib/modules/messaging/legacy-api.ts';
			const { chatApi } = await import(path);
			const command = chatApi.command.bind(chatApi);
			let started!: () => void;
			let release!: () => void;
			const began = new Promise<void>((resolve) => (started = resolve));
			const gate = new Promise<void>((resolve) => (release = resolve));
			(window as unknown as { heldChatCommand: { started: Promise<void>; release: () => void } }).heldChatCommand = { started: began, release };
			chatApi.command = async (id: string, call: { method: string }) => {
				if (call.method === method) {
					chatApi.command = command;
					started(); await gate;
				}
				return command(id, call);
			};
		}, kind === 'channel' ? 'channel.create' : 'dm.open');
		await side(page).getByRole('button', { name: kind === 'channel' ? 'Add channel' : 'New message', exact: true }).click();
		const dialog = page.getByRole('dialog', { name: kind === 'channel' ? 'Add a channel' : 'New message', exact: true });
		if (kind === 'channel') await dialog.getByLabel('Name').fill('pending-channel');
		else await dialog.getByRole('checkbox', { name: /Inês Duarte/ }).check();
		await dialog.getByRole('button', { name: kind === 'channel' ? 'Add channel' : 'Open', exact: true }).click();
		await page.evaluate(() => (window as unknown as { heldChatCommand: { started: Promise<void> } }).heldChatCommand.started);
		// Both popstates complete: Back, then SvelteKit's restoration after the modal cancels navigation.
		await page.evaluate(() => new Promise<void>((resolve) => {
			let count = 0;
			const returned = () => { if (++count === 2) { removeEventListener('popstate', returned); resolve(); } };
			addEventListener('popstate', returned);
			history.back();
		}));
		await expect(dialog).toBeVisible();
		await expect(page.getByTestId('conversation-title')).toHaveText('#release-v1');
		await expect(page).toHaveURL(/c=c-release-v1/);
		await expect(dialog).toHaveAttribute('data-state', 'open');
		await shot(page, `chat-back-keeps-pending-${kind}`);
		await expect(dialog).toHaveAttribute('data-state', 'open');
		await page.evaluate(() => (window as unknown as { heldChatCommand: { release: () => void } }).heldChatCommand.release());
		await expect(dialog).toBeHidden();
		await expect(page.getByTestId('conversation-title')).toHaveText(kind === 'channel' ? '#pending-channel' : 'Inês Duarte');
	});
}

for (const place of ['conversation', 'thread'] as const) {
	test(`CHAT-14 BUG-57: mouse Retry still focuses the delivered ${place} message`, async ({ page }) => {
		await open(page, '?chat=fail'); await go(page, 'release-v1');
		if (place === 'thread') await page.getByTestId('replies-link').first().click();
		const area = place === 'thread' ? page.getByTestId('thread-panel') : page.getByRole('region', { name: 'Conversation', exact: true });
		const composer = area.getByTestId('composer');
		const text = `Pointer retry in the ${place}`;
		await composer.fill(text); await composer.press('Control+Enter');
		const failed = area.getByTestId('message').filter({ hasText: text });
		await expect(failed.getByTestId('not-delivered')).toBeVisible();
		await failed.getByRole('button', { name: 'Retry', exact: true }).click();
		await expect(area.getByTestId('not-delivered')).toHaveCount(0);
		const delivered = area.getByTestId('message').filter({ hasText: text });
		await expect(delivered).toHaveCount(1);
		await expect(delivered).toBeFocused();
		await shot(page, `chat-pointer-retry-${place}-focus`);
	});
}
