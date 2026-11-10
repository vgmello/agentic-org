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
