import { expect, test, type Page } from '@playwright/test';
import { visit } from './ready';
import { shot } from './shot';

async function open(page: Page) {
	await visit(page, '/create');
	await page.getByLabel('Organization name').fill('Acme');
	await page.getByRole('button', { name: 'Create organization' }).click();
	await expect(page).toHaveURL(/\/agents$/);
	await visit(page, '/chat');
	await expect(page.getByRole('navigation', { name: 'Chat' })).toBeVisible();
}
const side = (page: Page) => page.getByRole('navigation', { name: 'Chat' });
const go = (page: Page, name: string) => side(page).getByTestId('conversation-link').filter({ hasText: name }).first().click();

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
		expect(await page.getByTestId('message-list').evaluate((element) => element.closest('[role="log"], [aria-live]') === null)).toBe(true);
	});
}

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
