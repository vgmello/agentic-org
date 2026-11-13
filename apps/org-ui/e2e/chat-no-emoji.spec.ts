import { expect, test, type Locator, type Page } from '@playwright/test';
import { visit } from './ready';
import { shot } from './shot';
import expectedIcons from './fixtures/heroicons-v2.2.0.json' with { type: 'json' };

declare global { interface Window { noEmojiReactions: Record<string, unknown>[]; } }


async function iconIs(icon: Locator, key: keyof typeof expectedIcons, sidebar = false) {
	const expected = expectedIcons[key];
	await expect(icon).toHaveCount(1);
	for (const [name, value] of Object.entries(expected.attributes)) await expect(icon).toHaveAttribute(name, value);
	const scale = await icon.evaluate(() => parseFloat(getComputedStyle(document.documentElement).fontSize) / 16);
	const naturalSize = Number(expected.attributes.viewBox.split(' ')[2]) * scale;
	const size = sidebar ? Math.min(naturalSize, 32) : naturalSize;
	await expect(icon).toHaveCSS('width', `${size}px`);
	await expect(icon).toHaveCSS('height', `${size}px`);
	expect(await icon.locator('path').evaluateAll((paths) => paths.map((path) => Object.fromEntries([...path.attributes].map((a) => [a.name, a.value]))))).toEqual(expected.paths);
}


async function navigationFits(nav: Locator) {
	const boxes = await nav.getByRole('button').evaluateAll((buttons) => buttons.map((button) => {
		const box = button.getBoundingClientRect(), walker = document.createTreeWalker(button, NodeFilter.SHOW_TEXT);
		const rects = [...button.querySelectorAll('svg')].map((svg) => svg.getBoundingClientRect());
		for (let node = walker.nextNode(); node; node = walker.nextNode()) {
			if (!node.textContent?.trim()) continue;
			const range = document.createRange(); range.selectNode(node); rects.push(...range.getClientRects());
		}
		return { name: button.textContent, fits: rects.every((rect) => rect.left >= box.left - 1 && rect.right <= box.right + 1) };
	}));
	expect(boxes.length).toBeGreaterThan(5);
	for (const box of boxes) expect(box.fits, `${box.name}: icons, labels and counters fit the row`).toBe(true);
	expect(await nav.evaluate((element) => element.scrollWidth - element.clientWidth)).toBeLessThanOrEqual(1);
}

async function noEmoji(scope: Locator) {
	const content = await scope.evaluate((element) => element.textContent + ' ' + [...element.querySelectorAll('*')]
		.flatMap((node) => ['aria-label', 'title', 'placeholder', 'alt'].map((name) => node.getAttribute(name) ?? '')).join(' '));
	expect(content.match(/\p{Emoji_Presentation}|\p{Extended_Pictographic}\uFE0F|\p{Regional_Indicator}{2}|[#*0-9]\uFE0F?\u20E3/gu) ?? []).toEqual([]);
}
async function open(page: Page) {
	await visit(page, '/create');
	await page.getByLabel('Organization name').fill('Acme');
	await page.getByRole('button', { name: 'Create organization' }).click();
	await expect(page).toHaveURL(/\/agents$/);
	await visit(page, '/chat');
	await expect(page.getByTestId('conversation-title')).toHaveText('Needs you');
}
async function conversations(page: Page) {
	const nav = page.getByRole('navigation', { name: 'Chat', exact: true });
	const inline = await page.locator('.chat-frame').evaluate((frame) => frame.getBoundingClientRect().width >= 768);
	if (!inline) {
		const toggle = page.getByRole('button', { name: /^Conversations(?:,|$)/ });
		await expect(toggle).toBeVisible();
		await toggle.click();
	}
	await expect(nav).toBeVisible();
	return nav;
}

for (const width of [320, 640, 960, 1280]) for (const scale of [1, 1.25, 2]) {
	test(`E1–E3 CHAT-1/5/8/14 BUG-70: Chat UI uses labelled icons at ${width}px and ${scale * 100}% text`, { tag: '@webkit' }, async ({ page }) => {
		await page.setViewportSize({ width, height: 900 });
		await page.addInitScript((scale) => {
			const apply = () => document.documentElement.style.setProperty('font-size', `${scale * 100}%`, 'important');
			addEventListener('DOMContentLoaded', apply, { once: true });
			if (document.documentElement) apply();
		}, scale);
		await open(page);
		await expect(page.locator('html')).toHaveCSS('font-size', `${scale * 16}px`);
		const nav = await conversations(page);
		await noEmoji(nav);
		await navigationFits(nav);
		for (const [id, name] of [['needs-you-link', 'outline:bell'], ['mentions-link', 'outline:at-symbol'], ['threads-link', 'outline:chat-bubble-left-right']] as const) {
			const icon = nav.getByTestId(id).locator('svg');
			await expect(icon).toHaveCount(1);
			await expect(icon).toHaveAttribute('aria-hidden', 'true');
			await iconIs(icon, name, true);
		}
		await iconIs(nav.locator('[data-conversation="c-general"] svg'), 'outline:hashtag', true);
		await iconIs(nav.locator('[data-conversation="c-group"] svg'), 'outline:user-group', true);
		await shot(page, `no-emoji-nav-${width}-${scale}`, { context: nav });
		await nav.getByTestId('needs-you-link').click();
		await noEmoji(page.getByTestId('ask-card').first());
		await iconIs(page.getByTestId('ask-card').filter({ hasText: 'Block bug reports' }).locator('p').first().locator('svg'), 'micro:question-mark-circle');
		await iconIs(page.getByTestId('ask-card').filter({ hasText: 'Publish the 0.2' }).locator('p').first().locator('svg'), 'micro:check-circle');
		await expect(page.getByTestId('ask-card').filter({ hasText: 'Publish the 0.2' })).toContainText('Approval');
		await noEmoji(page.locator('body'));
		await shot(page, `no-emoji-asks-${width}-${scale}`);
		await visit(page, '/chat?c=c-release-v1');
		await expect(page.locator('html')).toHaveCSS('font-size', `${scale * 16}px`);
		await expect(page.getByTestId('handoff-card')).toBeVisible();
		await noEmoji(page.getByTestId('handoff-card'));
		await iconIs(page.getByTestId('handoff-card').locator('p').first().locator('svg'), 'micro:arrows-right-left');
		await noEmoji(page.locator('body'));
		await shot(page, `no-emoji-handoff-${width}-${scale}`);
		await visit(page, '/chat?c=c-general');
		await expect(page.locator('html')).toHaveCSS('font-size', `${scale * 16}px`);
		const row = page.getByTestId('message').filter({ hasText: 'Ready for review: create-org' });
		await expect(row).toBeVisible();
		await noEmoji(row);
		await iconIs(row.getByTitle('Announcement', { exact: true }).locator('svg'), 'micro:megaphone');
		await page.evaluate(async () => {
			const path = '/src/lib/modules/messaging/legacy-api.ts';
			const { chatApi } = await import(path);
			const command = chatApi.command.bind(chatApi);
			window.noEmojiReactions = [];
			chatApi.command = (organization: string, call: { method: string }) => {
				if (call.method === 'chat.react') window.noEmojiReactions.push(JSON.parse(JSON.stringify(call)));
				return command(organization, call);
			};
		});
		const seen = row.getByRole('button', { name: 'Seen, 1', exact: true });
		await iconIs(seen.locator('svg'), 'micro:check-circle');
		await seen.click();
		await expect(row.getByRole('button', { name: 'Seen, 2', exact: true })).toHaveAttribute('aria-pressed', 'true');
		await row.getByRole('button', { name: 'React Looking', exact: true }).focus();
		await row.getByRole('button', { name: 'React Looking', exact: true }).press('Enter');
		await expect(row.getByRole('button', { name: 'Looking, 1', exact: true })).toHaveAttribute('aria-pressed', 'true');
		await iconIs(row.getByRole('button', { name: 'Looking, 1', exact: true }).locator('svg'), 'micro:eye');
		const messageId = await row.getAttribute('data-id');
		await expect.poll(() => page.evaluate(() => window.noEmojiReactions)).toEqual([
			{ method: 'chat.react', message_id: messageId, reaction: 'seen', change_id: expect.any(String) },
			{ method: 'chat.react', message_id: messageId, reaction: 'looking', change_id: expect.any(String) }
		]);
		await row.getByTestId('message-menu').click();
		await page.getByRole('menuitem', { name: 'Pin to the conversation', exact: true }).click();
		await expect(row.getByTitle('Pinned', { exact: true })).toHaveText('Pinned');
		await expect(row.getByTestId('replies-link')).toHaveText('3 replies · Sam Okafor posted 3 more updates');
		await shot(page, `no-emoji-message-${width}-${scale}`, { context: row });
		await page.getByLabel('Attach files').setInputFiles({ name: 'plan.txt', mimeType: 'text/plain', buffer: Buffer.from('plan') });
		const attached = page.getByRole('list', { name: 'Attached', exact: true });
		await expect(attached).toContainText('plan.txt');
		await noEmoji(attached);
		await iconIs(attached.getByRole('listitem').locator(':scope > svg'), 'micro:paper-clip');
		await iconIs(attached.getByRole('button').locator('svg'), 'mini:x-mark');
		await iconIs(page.getByRole('button', { name: 'Attach', exact: true }).locator('svg'), 'mini:paper-clip');
		await expect(attached.getByRole('button', { name: 'Remove plan.txt', exact: true })).toHaveAttribute('title', 'Remove plan.txt');
		await shot(page, `no-emoji-attached-${width}-${scale}`, { context: attached });
		await page.getByTestId('composer').fill('Attachment ready');
		await page.getByTestId('composer').press('Control+Enter');
		const sent = page.getByTestId('message').filter({ hasText: 'Attachment ready' });
		await expect(sent.getByRole('list', { name: 'Attachments' })).toContainText('plan.txt');
		await noEmoji(sent);
		await iconIs(sent.getByRole('list', { name: 'Attachments' }).locator('svg'), 'micro:paper-clip');
		await noEmoji(page.locator('body'));
		await shot(page, `no-emoji-sent-${width}-${scale}`, { context: sent });
	});
}
