import { expect, test, type Page } from '@playwright/test';
import { visit } from './ready';
import { shot } from './shot';

async function open(page: Page, width: number) {
	await visit(page, '/create');
	await page.getByLabel('Organization name').fill('Acme');
	await page.getByRole('button', { name: 'Create organization' }).click();
	await expect(page).toHaveURL(/\/agents$/);
	await page.setViewportSize({ width, height: 720 });
	await visit(page, '/chat?c=c-release-v1');
	await expect(page.getByTestId('conversation-title')).toHaveText('#release-v1');
}

for (const width of [1440, 960, 320]) {
	for (const close of ['Close', 'Escape'] as const) {
		test(`CHAT-31 BUG-65: ${close} restores a mouse-opened Thread to its replies button at ${width}px`, { tag: '@webkit' }, async ({ page }) => {
			await open(page, width);
			const opener = page.getByTestId('message-list').getByTestId('replies-link').first();
			const originalOpener = await opener.elementHandle();
			await opener.click();
			const pane = page.getByTestId('thread-panel');
			await expect(pane).toBeVisible();
			if (close === 'Close') await pane.getByRole('button', { name: 'Close thread', exact: true }).click();
			else await page.keyboard.press('Escape');
			await expect(pane).toHaveCount(0);
			await expect(opener).toBeFocused();
			await expect.poll(() => page.evaluate((element) => document.activeElement === element, originalOpener)).toBe(true);
			await expect.poll(() => opener.evaluate((element) => {
				const box = element.getBoundingClientRect();
				const header = document.querySelector('[data-shell-header]')!.getBoundingClientRect();
				return box.top >= header.bottom && box.bottom <= innerHeight
					&& element.contains(document.elementFromPoint(box.x + box.width / 2, box.y + box.height / 2));
			})).toBe(true);
			await shot(page, `chat-thread-mouse-${close.toLowerCase()}-return-${width}`);
		});
	}
}

for (const width of [1440, 960]) {
	test(`CHAT-31 BUG-65: a keyboard-opened Thread returns visible focus at ${width}px`, { tag: '@webkit' }, async ({ page }) => {
		await open(page, width);
		const opener = page.getByTestId('message-list').getByTestId('replies-link').first();
		const originalOpener = await opener.elementHandle();
		await page.keyboard.press('Tab');
		await opener.focus();
		await opener.press('Enter');
		await expect(page.getByTestId('thread-panel')).toBeVisible();
		await page.keyboard.press('Escape');
		await expect(page.getByTestId('thread-panel')).toHaveCount(0);
		await expect(opener).toBeFocused();
		await expect.poll(() => page.evaluate((element) => document.activeElement === element, originalOpener)).toBe(true);
		await expect.poll(() => opener.evaluate((element) => {
			const style = getComputedStyle(element);
			return element.matches(':focus-visible') && style.outlineStyle !== 'none' && Number.parseFloat(style.outlineWidth) > 0;
		})).toBe(true);
		await shot(page, `chat-thread-keyboard-return-${width}`);
	});
}

for (const trigger of ['toolbar', 'handoff'] as const) {
	test(`CHAT-31 BUG-65: Thread returns to its ${trigger} opening button`, { tag: '@webkit' }, async ({ page }) => {
		await open(page, 960);
		const messages = page.getByTestId('message-list').getByTestId('message');
		const message = messages.filter({ hasText: trigger === 'handoff' ? 'Please review the store traits' : 'your part and status for Phase 1' });
		await message.hover();
		const opener = message.getByRole('button', { name: trigger === 'handoff' ? 'Reply…' : 'Reply in thread', exact: true });
		const originalOpener = await opener.elementHandle();
		await opener.click();
		const pane = page.getByTestId('thread-panel');
		await expect(pane).toBeVisible();
		await pane.getByRole('button', { name: 'Close thread', exact: true }).click();
		await expect(pane).toHaveCount(0);
		await expect(opener).toBeFocused();
		await expect.poll(() => page.evaluate((element) => document.activeElement === element, originalOpener)).toBe(true);
		if (trigger === 'toolbar') await expect(message.getByRole('toolbar', { name: 'Message actions' })).toHaveCSS('opacity', '1');
		await shot(page, `chat-thread-${trigger}-return`);
	});
}

test('CHAT-31 BUG-65: Close restores the original opener after a reply re-renders the thread', { tag: '@webkit' }, async ({ page }) => {
	await open(page, 960);
	const opener = page.getByTestId('message-list').getByTestId('replies-link').first();
	const originalOpener = await opener.elementHandle();
	const previousCount = await opener.innerText();
	await opener.click();
	const pane = page.getByTestId('thread-panel');
	const composer = pane.getByTestId('composer');
	await composer.fill('A delivered reply keeps the original opener');
	await composer.press('Control+Enter');
	await expect(pane.getByTestId('message-body').filter({ hasText: 'A delivered reply keeps the original opener' })).toBeVisible();
	await expect(opener).not.toHaveText(previousCount);
	await pane.getByRole('button', { name: 'Close thread', exact: true }).click();
	await expect(pane).toHaveCount(0);
	await expect.poll(() => page.evaluate((element) => document.activeElement === element, originalOpener)).toBe(true);
	await shot(page, 'chat-thread-original-opener-after-reply');
});
