import { expect, test, type Locator, type Page } from '@playwright/test';
import { visit } from './ready';
import { shot } from './shot';

const side = (page: Page) => page.getByRole('navigation', { name: 'Chat', exact: true });
const mainNav = (page: Page) => page.getByRole('navigation', { name: 'Main', exact: true });
const thread = (page: Page) => page.getByTestId('thread-panel');
const fullTitle = (place: string) => `${place} · Chat · Agentic Org`;

async function open(page: Page, search = '') {
	await visit(page, '/create');
	await page.getByLabel('Organization name').fill('Acme');
	await page.getByRole('button', { name: 'Create organization' }).click();
	await expect(page).toHaveURL(/\/agents$/);
	await visit(page, `/chat${search}`);
	await expect(side(page)).toBeVisible();
}

async function channel(page: Page, id: string, title: string) {
	await side(page).locator(`[data-conversation="${id}"]`).click();
	await expect(page.getByTestId('conversation-title')).toHaveText(title);
}

async function openReleaseThread(page: Page) {
	await channel(page, 'c-release-v1', '#release-v1');
	await page.getByTestId('replies-link').first().click();
	await expect(thread(page)).toBeVisible();
}

async function restored(field: Locator, text: string) {
	await expect(field).toHaveValue(text);
	await expect.poll(() => field.evaluate((element: HTMLTextAreaElement) => [element.selectionStart, element.selectionEnd])).toEqual([text.length, text.length]);
}

test('CHAT-32 CHAT-33 BUG-62: each history entry names its own Chat place', async ({ page, browserName }) => {
	await open(page);
	await expect(page).toHaveTitle(fullTitle('Needs you'));
	await side(page).getByTestId('mentions-link').click();
	await expect(page).toHaveTitle(fullTitle('Mentions'));
	await channel(page, 'c-general', '#general');
	await expect(page).toHaveTitle(fullTitle('#general'));
	await expect(page).toHaveURL(/\?c=c-general$/);
	if (browserName === 'chromium') {
		const session = await page.context().newCDPSession(page);
		const { entries } = await session.send('Page.getNavigationHistory');
		await session.detach();
		expect(entries.filter((entry) => new URL(entry.url).pathname === '/chat').map(({ url, title }) => [new URL(url).search, title])).toEqual([
			['', fullTitle('Needs you')],
			['?v=mentions', fullTitle('Mentions')],
			['?c=c-general', fullTitle('#general')]
		]);
	}
	await page.goBack();
	await expect(page).toHaveTitle(fullTitle('Mentions'));
	await page.goForward();
	await expect(page).toHaveTitle(fullTitle('#general'));
	await shot(page, 'chat-history-own-place-titles');
});

test('CHAT-34 CHAT-35 BUG-63: stopped office disables channel and DM management', async ({ page }) => {
	await open(page, '?chat=offline');
	await channel(page, 'c-release-v1', '#release-v1');
	await expect(page.getByTestId('office-down')).toBeVisible();
	await page.getByRole('button', { name: 'Members', exact: true }).click();
	const members = page.getByTestId('members-panel');
	const controls = [side(page).getByRole('button', { name: 'Add channel', exact: true }), side(page).getByRole('button', { name: 'New message', exact: true }), members.getByRole('button', { name: /^Remove .* from the channel$/ }).first(), ...['Add people…', 'Rename…', 'Archive'].map((name) => members.getByRole('button', { name, exact: true }))];
	await page.evaluate(async () => {
		const path = '/src/lib/modules/messaging/legacy-api.ts';
		const { chatApi } = await import(path);
		const command = chatApi.command.bind(chatApi);
		document.documentElement.dataset.chatManagementWrites = '0';
		chatApi.command = async (id: string, call: { method: string }) => {
			if (call.method.startsWith('channel.') || call.method === 'dm.open') document.documentElement.dataset.chatManagementWrites = String(Number(document.documentElement.dataset.chatManagementWrites) + 1);
			return command(id, call);
		};
	});
	for (const control of controls) {
		await expect(control).toBeDisabled();
		await control.dispatchEvent('click');
	}
	await expect(page.getByRole('dialog')).toHaveCount(0);
	await expect(page.locator('html')).toHaveAttribute('data-chat-management-writes', '0');
	await shot(page, 'chat-stopped-management-disabled');
});

for (const kind of ['channel', 'DM'] as const) {
	test(`CHAT-34 BUG-63: an open ${kind} dialog cannot submit after the office stops`, async ({ page }) => {
		await open(page);
		await mainNav(page).getByRole('link', { name: 'Agents', exact: true }).click();
		await expect(page).toHaveURL(/\/agents$/);
		await page.evaluate(async () => {
			const path = '/src/lib/modules/messaging/legacy-api.ts';
			const { chatApi } = await import(path);
			const snapshot = chatApi.snapshot.bind(chatApi);
			const subscribe = chatApi.subscribe.bind(chatApi);
			const command = chatApi.command.bind(chatApi);
			let stopped = false;
			let refresh: (() => void) | undefined;
			document.documentElement.dataset.chatManagementWrites = '0';
			chatApi.snapshot = async (id: string) => ({ ...await snapshot(id), office_running: !stopped });
			chatApi.subscribe = (id: string, listener: () => void) => {
				refresh = listener;
				return subscribe(id, listener);
			};
			chatApi.command = async (id: string, call: { method: string }) => {
				if (call.method.startsWith('channel.') || call.method === 'dm.open') document.documentElement.dataset.chatManagementWrites = String(Number(document.documentElement.dataset.chatManagementWrites) + 1);
				return command(id, call);
			};
			(window as Window & { stopTestChatOffice?: () => void }).stopTestChatOffice = () => {
				stopped = true;
				refresh?.();
			};
		});
		await mainNav(page).getByRole('link', { name: 'Chat', exact: true }).click();
		await side(page).getByRole('button', { name: kind === 'channel' ? 'Add channel' : 'New message', exact: true }).click();
		const dialog = page.getByRole('dialog', { name: kind === 'channel' ? 'Add a channel' : 'New message', exact: true });
		if (kind === 'channel') await dialog.getByLabel('Name').fill('offline-channel');
		else await dialog.getByRole('checkbox', { name: /Inês Duarte/ }).check();
		await page.evaluate(() => (window as Window & { stopTestChatOffice?: () => void }).stopTestChatOffice?.());
		await expect(page.getByTestId('office-down')).toBeVisible();
		await dialog.getByRole('button', { name: kind === 'channel' ? 'Add channel' : 'Open', exact: true }).click();
		await expect(dialog.getByRole('alert')).toHaveText("The office isn't running. Restart it from Offices.");
		await expect(page.locator('html')).toHaveAttribute('data-chat-management-writes', '0');
		await shot(page, `chat-stopped-${kind.toLowerCase()}-dialog`);
	});
}

for (const focus of ['Send', 'Also send'] as const) {
	test(`CHAT-27 CHAT-28: Escape from ${focus} closes Thread and preserves the reply`, async ({ page }) => {
		await open(page);
		await openReleaseThread(page);
		const composer = thread(page).getByTestId('composer');
		const text = `Unsent from ${focus}`;
		await composer.fill(text);
		const control = focus === 'Send' ? thread(page).getByRole('button', { name: /^Send/ }) : thread(page).getByRole('checkbox', { name: /^Also send/ });
		await control.press('Escape');
		await expect(thread(page)).toHaveCount(0);
		await page.getByTestId('replies-link').first().click();
		await restored(composer, text);
		await shot(page, `chat-restored-after-escape-${focus.toLowerCase().replace(' ', '-')}`);
	});
}

for (const action of ['Reply', 'Reject'] as const) {
	test(`CHAT-27: Close and thread switching keep the ask ${action} draft and thread reply separate`, async ({ page }) => {
		await open(page);
		await channel(page, 'c-project-atlas', '#atlas');
		const firstAsk = action === 'Reply' ? 'Block bug reports' : 'Publish the 0.2 release notes';
		const otherAsk = action === 'Reply' ? 'Publish the 0.2 release notes' : 'Block bug reports';
		const openAskThread = async (body: string) => {
			const message = page.getByTestId('message-list').getByTestId('message').filter({ hasText: body });
			await message.hover();
			await message.getByRole('button', { name: 'Reply in thread', exact: true }).click();
		};
		await openAskThread(firstAsk);
		const composer = thread(page).getByTestId('composer');
		const card = thread(page).getByTestId('ask-card');
		const reply = card.getByRole('button', { name: `${action}…`, exact: true });
		await composer.fill('The thread reply');
		await reply.click();
		await card.getByRole('textbox').fill('The separate answer');
		await thread(page).getByRole('button', { name: 'Close thread', exact: true }).click();
		await expect(thread(page)).toHaveCount(0);
		await openAskThread(firstAsk);
		await restored(composer, 'The thread reply');
		await reply.click();
		await restored(card.getByRole('textbox'), 'The separate answer');
		await openAskThread(otherAsk);
		await expect(composer).toHaveValue('');
		await composer.fill('A different thread draft');
		await openAskThread(firstAsk);
		await restored(composer, 'The thread reply');
		await reply.click();
		await restored(card.getByRole('textbox'), 'The separate answer');
		await shot(page, `chat-restored-thread-and-ask-${action.toLowerCase()}`);
	});
}

test('CHAT-27: thread draft survives same-tab navigation, then send and reload clear it', async ({ page }) => {
	await open(page);
	await openReleaseThread(page);
	const composer = thread(page).getByTestId('composer');
	await composer.fill('Still here on return');
	await mainNav(page).getByRole('link', { name: 'Agents', exact: true }).click();
	await expect(page).toHaveURL(/\/agents$/);
	await mainNav(page).getByRole('link', { name: 'Chat', exact: true }).click();
	await expect(side(page)).toBeVisible();
	await openReleaseThread(page);
	await restored(composer, 'Still here on return');
	await composer.press('Control+Enter');
	await expect(thread(page).getByTestId('message-body').filter({ hasText: 'Still here on return' })).toBeVisible();
	await thread(page).getByRole('button', { name: 'Close thread', exact: true }).click();
	await page.getByTestId('replies-link').first().click();
	await expect(composer).toHaveValue('');
	await composer.fill('Cleared on full reload');
	await expect(page).toHaveURL(/thread=/);
	await page.reload();
	await expect(composer).toHaveValue('');
	await shot(page, 'chat-reload-clears-thread-draft');
});

test('CHAT-26 BUG-45d: punctuation after a slash word stays literal', async ({ page }) => {
	await open(page);
	await channel(page, 'c-general', '#general');
	const composer = page.getByTestId('composer');
	for (const text of ['/bug: this is text', '/status.']) {
		await composer.fill(text);
		await composer.press('Control+Enter');
		await expect(page.getByTestId('message-body').filter({ hasText: text })).toHaveText(text);
		await expect(page.getByRole('dialog')).toHaveCount(0);
	}
	await shot(page, 'chat-slash-punctuation-is-literal');
});
test('CHAT-34 BUG-63: disabled management leaves the Conversations overlay open', async ({ page }) => {
	await open(page, '?chat=offline');
	await page.setViewportSize({ width: 960, height: 720 });
	const toggle = page.getByRole('button', { name: /^Conversations/ });
	await toggle.press('Enter');
	const sheet = page.getByRole('dialog', { name: 'Conversations', exact: true });
	for (const name of ['Add channel', 'New message']) {
		const action = sheet.getByRole('button', { name, exact: true });
		await expect(action).toBeDisabled();
		await action.dispatchEvent('click');
		await expect(sheet).toBeVisible();
	}
	await expect(page.getByRole('dialog')).toHaveCount(1);
	await shot(page, 'chat-stopped-conversation-sheet');
});

test('CHAT-32 CHAT-33 BUG-62: Search has one history entry and results restore through Back', async ({ page, browserName }) => {
	await open(page);
	await channel(page, 'c-general', '#general');
	await expect(page).toHaveURL(/\?c=c-general$/);
	const initialLength = await page.evaluate(() => history.length);
	const search = page.getByRole('searchbox', { name: 'Search chat' });
	await search.pressSequentially('has:ask');
	await expect.poll(() => new URL(page.url()).searchParams.get('q')).toBe('has:ask');
	await expect(page).toHaveTitle(fullTitle('Search'));
	const query = 'has:ask in:#atlas';
	await search.pressSequentially(' in:#atlas');
	await expect.poll(() => new URL(page.url()).searchParams.get('q')).toBe(query);
	await expect(search).toBeFocused();
	expect(await page.evaluate(() => history.length)).toBe(initialLength + 1);
	if (browserName === 'chromium') {
		const session = await page.context().newCDPSession(page);
		const { entries } = await session.send('Page.getNavigationHistory');
		await session.detach();
		const channelEntry = entries.find((entry) => new URL(entry.url).search === '?c=c-general');
		expect(channelEntry?.title).toBe(fullTitle('#general'));
	}
	await page.goBack();
	await expect(page).toHaveURL(/\?c=c-general$/);
	await expect(page).toHaveTitle(fullTitle('#general'));
	await expect(search).toHaveValue('');
	await page.goForward();
	await expect(search).toHaveValue(query);
	await expect(page).toHaveTitle(fullTitle('Search'));
	await page.reload();
	await expect(search).toHaveValue(query);
	await expect(page).toHaveTitle(fullTitle('Search'));
	const result = page.getByTestId('message-list').getByTestId('message').filter({ hasText: 'Block bug reports' });
	await result.getByRole('button', { name: 'Open in conversation', exact: true }).click();
	await expect(page).toHaveTitle(fullTitle('#atlas'));
	await expect(page).toHaveURL(/c=c-project-atlas.*m=/);
	await expect(search).toHaveValue('');
	await page.goBack();
	await expect(search).toHaveValue(query);
	await expect(result).toBeVisible();
	await shot(page, 'chat-history-search-results-restored');
});

test('CHAT-32 CHAT-33 BUG-62: clearing Search returns to its place without another history entry', async ({ page }) => {
	await open(page);
	await channel(page, 'c-general', '#general');
	await expect(page).toHaveURL(/\?c=c-general$/);
	const search = page.getByRole('searchbox', { name: 'Search chat' });
	await search.fill('has:ask');
	await expect(page).toHaveTitle(fullTitle('Search'));
	await search.fill('');
	await expect(page).toHaveURL(/\?c=c-general$/);
	await expect(page).toHaveTitle(fullTitle('#general'));
	await page.goBack();
	await expect(page).toHaveTitle(fullTitle('Needs you'));
	await visit(page, '/chat?c=c-general&q=has%3Aask');
	await expect(search).toHaveValue('has:ask');
	await search.fill('');
	await expect(page).toHaveURL(/\?c=c-general$/);
	await expect(page).toHaveTitle(fullTitle('#general'));
	await shot(page, 'chat-history-search-cleared');
});

test('CHAT-32 CHAT-33 BUG-62: Back and reload resolve the same empty-organization place', async ({ page }) => {
	await open(page, '?org=empty');
	await expect(page).toHaveTitle(fullTitle('#general'));
	await side(page).getByTestId('mentions-link').click();
	await expect(page).toHaveTitle(fullTitle('Mentions'));
	await page.goBack();
	await expect(page).toHaveURL(/\?org=empty$/);
	await expect(page).toHaveTitle(fullTitle('#general'));
	await page.reload();
	await expect(page).toHaveTitle(fullTitle('#general'));
	await side(page).getByRole('button', { name: 'Needs you', exact: true }).click();
	await expect(page).toHaveURL(/v=needs/);
	await page.reload();
	await expect(page).toHaveTitle(fullTitle('Needs you'));
	await shot(page, 'chat-history-explicit-empty-needs-you');
});
