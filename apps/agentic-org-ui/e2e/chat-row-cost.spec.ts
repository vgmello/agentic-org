import { expect, test } from '@playwright/test';
import { buttonTabKey, openChat } from './chat-followups';
import { shot } from './shot';

test('MEM-2 CHAT-14: the resting action entry is accessible and keyboard use keeps its identity', { tag: '@webkit' }, async ({ page }) => {
	await openChat(page, 1280, 1, '/chat?c=c-general');
	await page.mouse.move(0, 0);
	const row = page.getByTestId('message').filter({ hasText: 'Ready for review: create-org' });
	const time = (await row.locator('time').textContent())!;
	const label = (await row.getAttribute('aria-label'))!;
	const who = label.slice(0, -`, ${time}`.length);
	const trigger = row.getByRole('button', { name: `Actions for ${who}'s message, ${time}`, exact: true });
	await expect(trigger).toHaveCount(1);
	await trigger.evaluate(node => node.dataset.memIdentity = 'kept');
	await page.keyboard.press(buttonTabKey(page));
	await trigger.focus();
	await expect(trigger).toBeFocused();
	await expect(trigger).toHaveCSS('outline-style', 'solid');
	await expect(trigger).toHaveCSS('outline-width', '2px');
	await shot(page, 'mem2-keyboard-entry', { context: row });
	for (const name of ['React Seen', 'React Looking', 'Reply in thread']) {
		await page.keyboard.press(buttonTabKey(page));
		await expect(row.getByRole('button', { name, exact: true })).toBeFocused();
	}
	await shot(page, 'mem2-keyboard-toolbar-order', { context: row });
	await trigger.focus();
	await trigger.press('Enter');
	await expect(page.getByRole('menu')).toBeVisible();
	await expect(trigger).toHaveAttribute('data-mem-identity', 'kept');
	await shot(page, 'mem2-open-menu');
	await page.keyboard.press('Escape');
	await expect(page.getByRole('menu')).toHaveCount(0);
	await expect(trigger).toBeFocused();
	await expect(trigger).toHaveAttribute('data-mem-identity', 'kept');
	await shot(page, 'mem2-menu-closed-trigger-retained', { context: row });
});

test('MEM-2 CHAT-14: the secondary toolbar mounts on hover or focus and leaves at rest', { tag: '@webkit' }, async ({ page }) => {
	await openChat(page, 1280, 1, '/chat?c=c-general');
	await page.mouse.move(0, 0);
	const row = page.getByTestId('message').filter({ hasText: 'Ready for review: create-org' });
	const react = row.getByRole('button', { name: 'React Seen', exact: true });
	await expect(row.getByTestId('message-menu')).toHaveCount(1);
	await expect(react).toHaveCount(0);
	await shot(page, 'mem2-resting-row', { context: row });
	await row.hover();
	await expect(react).toBeVisible();
	await shot(page, 'mem2-hover-toolbar', { context: row });
	await page.mouse.move(0, 0);
	await expect(react).toHaveCount(0);
	await row.getByTestId('message-menu').focus();
	await expect(react).toBeVisible();
	await page.getByRole('navigation', { name: 'Main' }).getByRole('link', { name: 'Chat', exact: true }).focus();
	await expect(react).toHaveCount(0);
	await expect(row.getByTestId('message-menu')).toHaveCount(1);
	await shot(page, 'mem2-toolbar-left-restored-row', { context: row });
});

for (const entry of ['keyboard Reply', 'menu Reply'] as const) {
	test(`MEM-2 CHAT-8/14: ${entry} retains the opener through a compact thread`, { tag: '@webkit' }, async ({ page }) => {
		await openChat(page, 960, 1, '/chat?c=c-general');
		const row = page.getByTestId('message-list').getByTestId('message').filter({ hasText: 'Ready for review: create-org' });
		const trigger = row.getByTestId('message-menu');
		await trigger.focus();
		const reply = row.getByRole('button', { name: 'Reply in thread', exact: true });
		const opener = entry === 'keyboard Reply' ? reply : trigger;
		const original = await opener.elementHandle();
		if (entry === 'menu Reply') {
			await trigger.press('Enter');
			await page.mouse.move(0, 0);
			await expect(row.locator('button[aria-label="React Seen"]')).toBeVisible();
			await page.getByRole('menuitem', { name: 'Reply in thread', exact: true }).click();
		} else {
			await reply.focus();
			await reply.press('Enter');
		}
		const panel = page.getByTestId('thread-panel');
		await expect(panel).toBeVisible();
		await panel.getByRole('button', { name: 'Close thread', exact: true }).focus();
		await page.mouse.move(0, 0);
		await expect.poll(() => original!.evaluate(node => node.isConnected)).toBe(true);
		await expect(row.locator('button[aria-label="React Seen"]')).toHaveCount(1);
		await panel.getByTestId('message').first().getByTestId('message-menu').click();
		await expect(page.getByRole('menuitem', { name: 'Reply in thread', exact: true })).toHaveCount(0);
		await page.keyboard.press('Escape');
		await shot(page, `mem2-${entry.replaceAll(' ', '-')}-thread-open`);
		await panel.getByRole('button', { name: 'Close thread', exact: true }).click();
		await expect(panel).toHaveCount(0);
		await expect(opener).toBeFocused();
		await expect.poll(() => original!.evaluate(node => document.activeElement === node)).toBe(true);
		await shot(page, `mem2-${entry.replaceAll(' ', '-')}-thread-return`, { context: row });
		await page.mouse.move(0, 0);
		await page.getByRole('navigation', { name: 'Main' }).getByRole('link', { name: 'Chat', exact: true }).focus();
		await expect(row.locator('button[aria-label="React Seen"]')).toHaveCount(0);
	});
}

for (const [into, title, finish] of [['bug', 'Report a bug', 'Cancel'], ['bug', 'Report a bug', 'Report bug'], ['request', 'Send a feature request', 'Cancel'], ['task', 'New task', 'Cancel']] as const) {
	test(`MEM-2 CHAT-9/14: a row dialog keeps its toolbar until ${into}/${finish} returns focus`, { tag: '@webkit' }, async ({ page }) => {
		await openChat(page, 1280, 1, '/chat?c=c-general');
		const row = page.getByTestId('message-list').getByTestId('message').filter({ hasText: 'Ready for review: create-org' });
		const trigger = row.getByTestId('message-menu');
		const original = await trigger.elementHandle();
		await trigger.focus();
		await trigger.press('Enter');
		await page.getByRole('menuitem', { name: `Turn into a ${into}…`, exact: true }).click();
		const dialog = page.getByRole('dialog', { name: title, exact: true });
		await expect(dialog).toBeVisible();
		await expect(dialog.getByRole('textbox').first()).toBeFocused();
		await page.mouse.move(0, 0);
		await expect(row.locator('button[aria-label="React Seen"]')).toHaveCount(1);
		await shot(page, `mem2-row-dialog-${into}-${finish.replaceAll(' ', '-')}-open`);
		await dialog.getByRole('button', { name: finish, exact: true }).click();
		await expect(dialog).toHaveCount(0);
		await expect(trigger).toBeFocused();
		await expect.poll(() => original!.evaluate(node => document.activeElement === node)).toBe(true);
		await shot(page, `mem2-row-dialog-${into}-${finish.replaceAll(' ', '-')}-return`, { context: row });
		await page.getByRole('navigation', { name: 'Main' }).getByRole('link', { name: 'Chat', exact: true }).focus();
		await expect(row.locator('button[aria-label="React Seen"]')).toHaveCount(0);
	});
}

for (const offline of [false, true]) {
	test(`MEM-2 CHAT-11/14: menu reactions preserve the ${offline ? 'offline' : 'running'} gates`, { tag: '@webkit' }, async ({ page }) => {
		await openChat(page, 1280, 1, `/chat?c=c-general${offline ? '&chat=offline' : ''}`);
		const row = page.getByTestId('message-list').getByTestId('message').filter({ hasText: 'Ready for review: create-org' });
		const trigger = row.getByTestId('message-menu');
		for (const reaction of ['Seen', 'Looking']) {
			await trigger.click();
			const item = page.getByRole('menuitem', { name: `React ${reaction}`, exact: true });
			if (offline) {
				await expect(item).toBeDisabled();
				await expect(row.getByRole('button', { name: `React ${reaction}`, exact: true })).toBeDisabled();
				await shot(page, `mem2-offline-menu-${reaction}`);
				await page.keyboard.press('Escape');
			} else {
				await expect(item).toBeEnabled();
				await item.click();
				await expect(row.getByRole('button', { name: `${reaction}, ${reaction === 'Seen' ? 2 : 1}`, exact: true })).toHaveAttribute('aria-pressed', 'true');
				await expect(trigger).toBeFocused();
				await shot(page, `mem2-menu-reacted-${reaction}`, { context: row });
			}
		}
	});
}

test('MEM-2 CHAT-14: read-only preview keeps its actionless rows', { tag: '@webkit' }, async ({ page }) => {
	await openChat(page, 1280, 1, '/chat?c=c-general&surface=bridge');
	const row = page.getByTestId('message').filter({ hasText: 'Ready for review: create-org' });
	await row.hover();
	await expect(row.getByTestId('message-menu')).toHaveCount(0);
	await expect(row.getByRole('toolbar', { name: 'Message actions' })).toHaveCount(0);
	await shot(page, 'mem2-preview-no-actions', { context: row });
});
