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

for (const panel of ['thread', 'members'] as const) {
	test(`CHAT-1/8 BUG-40: ${panel} overlays without squeezing the conversation at 960x640`, async ({ page }) => {
		await open(page);
		await go(page, 'release-v1');
		if (panel === 'thread') await page.getByTestId('replies-link').first().click();
		else await page.getByRole('button', { name: 'Members', exact: true }).click();
		await page.setViewportSize({ width: 960, height: 640 });
		const pane = page.getByTestId(`${panel}-panel`);
		await expect(pane).toBeVisible();
		await shot(page, `chat-${panel}-minimum`);
		const composer = page.getByRole('region', { name: 'Conversation', exact: true }).getByTestId('composer');
		await expect.poll(async () => (await composer.boundingBox())?.width ?? 0).toBeGreaterThan(300);
		const bounds = await pane.boundingBox();
		expect(bounds).not.toBeNull();
		expect(bounds!.x + bounds!.width).toBeLessThanOrEqual(960);
		expect(bounds!.y + bounds!.height).toBeLessThanOrEqual(640);
		await pane.getByRole('button', { name: `Close ${panel}` }).click();
		await expect(pane).toBeHidden();
		const search = page.getByRole('searchbox', { name: 'Search chat' });
		const searchBounds = await search.boundingBox();
		expect(searchBounds!.x + searchBounds!.width).toBeLessThanOrEqual(960);
		await composer.fill('Still room to write');
		await composer.press('Control+Enter');
		await expect(page.getByTestId('message-list')).toContainText('Still room to write');
		await shot(page, `chat-${panel}-minimum-closed`);
	});
}

test('CHAT-5/6 BUG-41: stopped office keeps asks and approvals read-only everywhere', async ({ page }) => {
	await open(page, '?chat=offline');
	await expect(page.getByTestId('office-down')).toBeVisible();
	for (const card of await page.getByTestId('ask-card').all()) {
		await expect(card.getByRole('button')).toHaveCount(0);
		await expect(card).toContainText("The office isn't running. Restart it from Offices.");
	}
	await shot(page, 'chat-offline-needs-you');
	await go(page, 'atlas');
	const asks = page.getByTestId('ask-card');
	await expect(asks.first()).toBeVisible();
	for (const card of await asks.all()) await expect(card.getByRole('button')).toHaveCount(0);
	await shot(page, 'chat-offline-conversation');
});

test('CHAT-7/8 BUG-42: incoming conversation and thread replies are in polite message logs', async ({ page }) => {
	await open(page);
	await go(page, 'Inês Duarte');
	const log = page.getByRole('log', { name: 'Messages', exact: true });
	await expect(log).toHaveAttribute('aria-live', 'polite');
	const composer = page.getByTestId('composer');
	await composer.fill('Could you help?');
	await composer.press('Control+Enter');
	await expect(log).toContainText('Sample reply');
	await expect(composer).toBeFocused();
	await shot(page, 'chat-announced-reply');
	await go(page, 'release-v1');
	await page.getByTestId('replies-link').first().click();
	const thread = page.getByTestId('thread-panel');
	const replies = thread.getByRole('log', { name: 'Thread messages' });
	await expect(replies).toHaveAttribute('aria-live', 'polite');
	await thread.getByTestId('composer').fill('An accessible thread reply');
	await thread.getByTestId('composer').press('Control+Enter');
	await expect(replies).toContainText('An accessible thread reply');
	await shot(page, 'chat-announced-thread-reply');
});

for (const action of ['option', 'reply', 'reject'] as const) {
	test(`CHAT-5 BUG-43: keyboard focus remains useful after ${action}`, async ({ page }) => {
		await open(page);
		const card = page.getByTestId('ask-card').filter({ hasText: action === 'reject' ? 'Publish the 0.2' : 'Block bug reports' });
		if (action !== 'option') {
			await card.getByRole('button', { name: action === 'reject' ? 'Reject…' : 'Reply…' }).click();
			await card.getByRole('textbox').fill('Please wait for the audit.');
		}
		const button = card.getByRole('button', { name: action === 'option' ? 'Yes, block' : action === 'reject' ? 'Reject' : 'Send reply', exact: true });
		await button.focus();
		await button.press('Enter');
		await expect(card).toHaveCount(0);
		await expect.poll(() => page.evaluate(() => document.activeElement?.tagName)).not.toBe('BODY');
		const focused = page.locator(':focus');
		await expect(page.getByTestId('needs-you-list')).toContainText('asks you');
		await expect(focused).toHaveRole('button');
		await shot(page, `chat-focus-${action}`);
		// Answer the final card: focus lands on the empty result rather than the document.
		const remaining = page.getByTestId('ask-card');
		const answer = remaining.getByRole('button', { name: action === 'reject' ? 'Yes, block' : 'Approve', exact: true });
		await answer.focus(); await answer.press('Enter');
		await expect(page.getByTestId('nothing-needs-you')).toBeFocused();
	});
}

test('CHAT-14 BUG-43: keyboard Retry focuses the delivered message', async ({ page }) => {
	await open(page, '?chat=fail'); await go(page, 'general');
	await page.getByTestId('composer').fill('Retry focus target');
	await page.getByTestId('composer').press('Control+Enter');
	const retry = page.getByRole('button', { name: 'Retry', exact: true });
	await expect(retry).toBeVisible(); await retry.focus(); await retry.press('Enter');
	await expect(page.getByTestId('not-delivered')).toHaveCount(0);
	const delivered = page.getByTestId('message').filter({ hasText: 'Retry focus target' });
	await expect(delivered).toHaveCount(1); await expect(delivered).toBeFocused();
	await shot(page, 'chat-focus-delivered');
});

test('CHAT-1/8/15 BUG-44: channel and thread survive Back, Forward and reload without losing mock parameters', async ({ page }) => {
	await open(page, '?chat=fail&surface=bridge');
	await go(page, 'general');
	await expect(page).toHaveURL(/c=c-general/);
	await go(page, 'release-v1');
	await expect(page).toHaveURL(/c=c-release-v1/);
	await page.getByTestId('replies-link').first().click();
	await expect(page).toHaveURL(/thread=/);
	const threadURL = page.url();
	await page.reload();
	await expect(page.getByTestId('conversation-title')).toHaveText('#release-v1');
	await expect(page.getByTestId('thread-panel')).toContainText('Storage is done');
	expect(new URL(page.url()).searchParams.get('chat')).toBe('fail');
	expect(new URL(page.url()).searchParams.get('surface')).toBe('bridge');
	await page.getByRole('button', { name: 'Close thread' }).click();
	await expect(page).not.toHaveURL(/thread=/);
	await page.goBack();
	await expect(page).toHaveURL(threadURL);
	await expect(page.getByTestId('thread-panel')).toBeVisible();
	await page.goBack();
	await expect(page.getByTestId('thread-panel')).toBeHidden();
	await page.goBack();
	await expect(page.getByTestId('conversation-title')).toHaveText('#general');
	await page.goForward();
	await expect(page.getByTestId('conversation-title')).toHaveText('#release-v1');
	await shot(page, 'chat-history-restored');
});

test('CHAT-2 BUG-45: editing a refused channel name clears its error and invalid state', async ({ page }) => {
	await open(page); await side(page).getByRole('button', { name: 'Add channel' }).click();
	const dialog = page.getByRole('dialog', { name: 'Add a channel' }); const field = dialog.getByLabel('Name');
	await field.fill('General'); await dialog.getByRole('button', { name: 'Add channel', exact: true }).click();
	await expect(dialog.getByRole('alert')).toContainText('already a channel');
	await expect(field).toHaveAttribute('aria-invalid', 'true');
	await field.fill('Release V2'); await expect(dialog.getByRole('alert')).toHaveCount(0);
	await expect(field).not.toHaveAttribute('aria-invalid', 'true');
	await shot(page, 'chat-channel-error-cleared');
	await dialog.getByRole('button', { name: 'Add channel', exact: true }).click();
	await expect(page.getByTestId('conversation-title')).toHaveText('#release-v2');
});

test('CHAT-14 BUG-45: over-limit UTF-8 text stays in the composer with actionable advice', async ({ page }) => {
	await open(page); await go(page, 'general'); const composer = page.getByTestId('composer');
	const text = 'é'.repeat(160000); await composer.fill(text); await composer.press('Control+Enter');
	await expect(composer).toHaveValue(text);
	await expect(page.getByRole('alert')).toContainText('Shorten it or attach it as a file');
	await expect(page.getByTestId('not-delivered')).toHaveCount(0);
	await expect(page.getByRole('button', { name: 'Retry', exact: true })).toHaveCount(0);
	await shot(page, 'chat-message-too-large');
	await composer.fill('Short enough now'); await composer.press('Control+Enter');
	await expect(page.getByTestId('message-list')).toContainText('Short enough now');
});

test('CHAT-8 BUG-45: failed thread replies have inline Retry and Dismiss, preserving also-in-channel', async ({ page }) => {
	await open(page, '?chat=fail'); await go(page, 'release-v1'); await page.getByTestId('replies-link').first().click();
	const thread = page.getByTestId('thread-panel'); const composer = thread.getByTestId('composer');
	await composer.fill('Keep this failed thread reply'); await thread.getByLabel('Also send to #release-v1').check();
	await composer.press('Control+Enter');
	const failed = thread.getByTestId('message').filter({ hasText: 'Keep this failed thread reply' });
	await expect(failed.getByTestId('not-delivered')).toContainText("The office didn't answer.");
	await shot(page, 'chat-thread-delivery-failed');
	await failed.getByRole('button', { name: 'Retry', exact: true }).click();
	await expect(thread.getByTestId('not-delivered')).toHaveCount(0);
	await expect(thread.getByTestId('message').filter({ hasText: 'Keep this failed thread reply' })).toHaveCount(1);
	await expect(page.getByTestId('message-list').getByTestId('message').filter({ hasText: 'Keep this failed thread reply' })).toHaveCount(1);
	await shot(page, 'chat-thread-delivery-retried');
	await composer.fill('Dismiss this thread failure'); await composer.press('Control+Enter');
	const dismissed = thread.getByTestId('message').filter({ hasText: 'Dismiss this thread failure' });
	await expect(dismissed.getByTestId('not-delivered')).toBeVisible();
	await dismissed.getByRole('button', { name: 'Dismiss', exact: true }).click();
	await expect(dismissed).toHaveCount(0);
	await expect(page.getByTestId('message-list')).not.toContainText('Dismiss this thread failure');
});

test('CHAT-9 BUG-45: an unknown slash command is refused and kept for editing', async ({ page }) => {
	await open(page); await go(page, 'general'); const composer = page.getByTestId('composer');
	await composer.fill('/frobnicate now'); await composer.press('Control+Enter');
	await expect(composer).toHaveValue('/frobnicate now');
	await expect(page.getByText('No command /frobnicate.', { exact: true })).toBeVisible();
	await expect(page.getByTestId('message-list')).not.toContainText('/frobnicate now');
	await shot(page, 'chat-unknown-command');
});

test('CHAT-14/18 BUG-45: hand-off task references include a project key and render as chips', async ({ page }) => {
	await open(page); await go(page, 'release-v1');
	const card = page.getByTestId('handoff-card').filter({ hasText: 'review the store traits' });
	await expect(card.getByTestId('item-chip')).toHaveText('ac-T-12');
	await shot(page, 'chat-handoff-item-chip');
});

for (const check of ['name', 'contrast'] as const) {
	test(`CHAT-3 BUG-45: selected DM status has sufficient ${check}`, async ({ page }) => {
		await open(page); await go(page, 'Sam Okafor');
		const row = side(page).locator('[data-conversation="c-dm-sam"]');
		if (check === 'name') {
			await expect(row).toHaveAccessibleName('Sam Okafor, Working');
			return;
		}
		const colors = await row.getByTestId('dm-status').evaluate(el => {
			const canvas = document.createElement('canvas'); canvas.width = canvas.height = 1;
			const ctx = canvas.getContext('2d')!;
			const rgb = (color: string) => { ctx.clearRect(0, 0, 1, 1); ctx.fillStyle = color; ctx.fillRect(0, 0, 1, 1); const c = Array.from(ctx.getImageData(0, 0, 1, 1).data); return [c[0], c[1], c[2], c[3] / 255]; };
			let background = [0, 0, 0];
			for (const node of [...Array.from((function* (n: Element | null): Generator<Element> { while (n) { yield n; n = n.parentElement; } })(el)).reverse()]) {
				const c = rgb(getComputedStyle(node).backgroundColor); const a = c[3] ?? 1;
				background = background.map((v, i) => c[i] * a + v * (1 - a));
			}
			return { foreground: rgb(getComputedStyle(el).color), background };
		});
		const lum = (c: number[]) => c.slice(0, 3).map(v => v / 255).map(v => v <= .04045 ? v / 12.92 : ((v + .055) / 1.055) ** 2.4).reduce((s, v, i) => s + v * [.2126, .7152, .0722][i], 0);
		const a = lum(colors.foreground), b = lum(colors.background); expect((Math.max(a, b) + .05) / (Math.min(a, b) + .05)).toBeGreaterThanOrEqual(4.5);
		await shot(page, 'chat-dm-readable-status');
	});
}

test('CHAT-8 BUG-45: Escape closes the thread and returns focus to its opener', async ({ page }) => {
	await open(page); await go(page, 'release-v1'); const opener = page.getByTestId('replies-link').first();
	await opener.focus(); await opener.press('Enter'); await expect(page.getByTestId('thread-panel')).toBeVisible();
	await page.keyboard.press('Escape'); await expect(page.getByTestId('thread-panel')).toBeHidden();
	await expect(opener).toBeFocused(); await shot(page, 'chat-thread-escape');
});

type HeldAnswer = { started: Promise<void>; release: () => void };

for (const movedOn of [false, true]) {
	test(`CHAT-5 BUG-43: a delayed refused answer ${movedOn ? 'preserves moved focus' : 'keeps focus for correction or retry'}`, async ({ page }) => {
		await open(page);
		await page.evaluate(async () => {
			const path = '/src/lib/modules/messaging/legacy-api.ts';
			const { chatApi } = await import(path);
			const command = chatApi.command.bind(chatApi);
			let started!: () => void;
			let release!: () => void;
			const began = new Promise<void>((resolve) => (started = resolve));
			const gate = new Promise<void>((resolve) => (release = resolve));
			(window as typeof window & { __heldChatAnswer: HeldAnswer }).__heldChatAnswer = { started: began, release };
			chatApi.command = async (id: string, call: { method: string }) => {
				if (call.method === 'chat.answer') {
					chatApi.command = command;
					started();
					await gate;
					throw new Error('Answer was refused. Try again.');
				}
				return command(id, call);
			};
		});
		const card = page.getByTestId('ask-card').filter({ hasText: 'Block bug reports' });
		const answer = card.getByRole('button', { name: 'Yes, block', exact: true });
		await answer.focus();
		await answer.press('Enter');
		await page.evaluate(() => (window as typeof window & { __heldChatAnswer: HeldAnswer }).__heldChatAnswer.started);
		await expect(answer).toBeDisabled();
		if (movedOn) await page.getByRole('searchbox', { name: 'Search chat' }).focus();
		await page.evaluate(() => (window as typeof window & { __heldChatAnswer: HeldAnswer }).__heldChatAnswer.release());
		await expect(card.getByRole('alert')).toHaveText('Answer was refused. Try again.');
		await expect(movedOn ? page.getByRole('searchbox', { name: 'Search chat' }) : answer).toBeFocused();
		await shot(page, movedOn ? 'chat-answer-refused-moved-focus' : 'chat-answer-refused-focus');
		await answer.focus();
		await answer.press('Enter');
		await expect(card).toHaveCount(0);
	});
}

test('CHAT-13/15 BUG-44: opening a search result uses its destination thread', async ({ page }) => {
	await open(page);
	await go(page, 'release-v1');
	await page.getByTestId('replies-link').first().click();
	await expect(page).toHaveURL(/thread=/);
	await page.getByRole('searchbox', { name: 'Search chat' }).fill('Delivered');
	const result = page.getByTestId('message-list').getByTestId('message').filter({ hasText: 'Delivered: the offices screen' });
	const messageId = await result.getAttribute('data-id');
	await result.getByRole('button', { name: 'Open in conversation' }).click();
	await expect(page.getByTestId('thread-panel')).toBeHidden();
	await expect.poll(() => new URL(page.url()).searchParams.get('m')).toBe(messageId);
	await expect(page).not.toHaveURL(/thread=/);
	await shot(page, 'chat-search-result-no-stale-thread');
	// A reply still opens its own thread, rather than losing legitimate message links.
	await page.getByRole('searchbox', { name: 'Search chat' }).fill('Storage is done');
	await page.getByTestId('message-list').getByRole('button', { name: 'Open in conversation' }).click();
	await expect(page.getByTestId('thread-panel')).toContainText('Storage is done');
	await shot(page, 'chat-search-result-reply-thread');
});
