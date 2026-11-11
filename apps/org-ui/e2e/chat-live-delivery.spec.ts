import { expect, test, type Page } from '@playwright/test';
import { visit } from './ready';
import { shot } from './shot';

type Announcement = { log: string; text: string };
declare global {
	interface Window {
		chatLiveProbe: { take: () => Announcement[] };
		releaseChatSend: () => void;
	}
}

// DOM offer-to-AT model, not a claim about VoiceOver speech. Observe real mutations:
// newly mounted logs are history, live=off overrides ancestors, and text is relevant
// only when requested. Keep the positive delivered-message assertions non-vacuous.
async function observe(page: Page) {
	await page.addInitScript(() => {
		let events: Announcement[] = [];
		const known = new WeakSet<Element>();
		const live = (node: Node): Element | null => {
			for (let e = node instanceof Element ? node : node.parentElement; e; e = e.parentElement) {
				const setting = e.getAttribute('aria-live');
				if (setting === 'off') return null;
				if (setting === 'polite' || setting === 'assertive' || e.getAttribute('role') === 'log') return e;
			}
			return null;
		};
		const textIn = (node: Node, region: Element) => {
			const text = (n: Node) => live(n) === region && !n.parentElement?.closest('[aria-hidden="true"], [inert], [hidden]') ? n.textContent ?? '' : '';
			if (node.nodeType === Node.TEXT_NODE) return text(node);
			const walker = document.createTreeWalker(node, NodeFilter.SHOW_TEXT);
			let result = '';
			for (let n = walker.nextNode(); n; n = walker.nextNode()) result += ` ${text(n)}`;
			return result;
		};
		const remember = () => document.querySelectorAll('[role="log"]').forEach((e) => known.add(e));
		const observer = new MutationObserver((changes) => {
			for (const change of changes) {
				const region = live(change.target);
				if (!region || region.getAttribute('role') !== 'log' || !known.has(region) || !region.isConnected) continue;
				const relevant = (region.getAttribute('aria-relevant') ?? 'additions text').split(/\s+/);
				const text = change.type === 'characterData'
					? relevant.includes('text') || relevant.includes('all') ? textIn(change.target, region) : ''
					: [...change.addedNodes].map((node) => relevant.includes('all') || relevant.includes(node.nodeType === Node.TEXT_NODE ? 'text' : 'additions') ? textIn(node, region) : '').join(' ');
				if (text.trim()) events.push({ log: region.getAttribute('aria-label') ?? '', text: text.replace(/\s+/g, ' ').trim() });
			}
			remember();
		});
		observer.observe(document, { subtree: true, childList: true, characterData: true });
		window.chatLiveProbe = { take: () => { const result = events; events = []; return result; } };
	});
}

async function take(page: Page) {
	// Cross a render boundary so mutation callbacks run before either a silent or positive assertion.
	await page.evaluate(() => new Promise<void>((resolve) => requestAnimationFrame(() => requestAnimationFrame(() => resolve()))));
	return page.evaluate(() => window.chatLiveProbe.take());
}

async function open(page: Page, id = 'c-release-v1') {
	await observe(page);
	await visit(page, '/create');
	await page.getByLabel('Organization name').fill('Acme');
	await page.getByRole('button', { name: 'Create organization' }).click();
	await expect(page).toHaveURL(/\/agents$/);
	await visit(page, `/chat?c=${id}`);
	await expect(page.getByRole('log', { name: 'Messages', exact: true })).toBeVisible();
	expect.soft(await take(page)).toEqual([]);
}

async function holdSend(page: Page) {
	await page.evaluate(async () => {
		const path = '/src/lib/modules/messaging/legacy-api.ts';
		const { chatApi } = await import(path);
		const command = chatApi.command.bind(chatApi);
		chatApi.command = async (id: string, call: { method: string }) => {
			if (call.method === 'msg.send') await new Promise<void>((resolve) => { window.releaseChatSend = resolve; });
			return command(id, call);
		};
	});
}

for (const place of ['conversation', 'thread'] as const) {
	test(`CHAT-1/8 BUG-66: ${place} offers a delivered message once, without its pending row or counters`, async ({ page }) => {
		await open(page);
		const root = page.getByTestId('message-list').getByTestId('replies-link').first();
		const previousCount = await root.innerText();
		if (place === 'thread') {
			await root.click();
			await expect(page.getByTestId('thread-panel')).toBeVisible();
			expect(await take(page)).toEqual([]);
		}
		await holdSend(page);
		const area = place === 'thread' ? page.getByTestId('thread-panel') : page.getByTestId('message-list');
		const composer = place === 'thread' ? area.getByTestId('composer') : page.getByTestId('composer');
		const body = `One delivered ${place} message`;
		await composer.fill(body);
		await composer.press('Control+Enter');
		await expect(area.getByTestId('sending')).toBeVisible();
		const pendingOffers = await take(page);
		await shot(page, `chat-live-${place}-pending`);
		await page.evaluate(() => window.releaseChatSend());
		await expect(area.getByTestId('sending')).toHaveCount(0);
		await expect(area.getByTestId('message-body').filter({ hasText: body })).toBeVisible();
		const deliveredOffers = await take(page);
		if (place === 'thread') {
			await expect(root).not.toHaveText(previousCount);
			await expect(area.getByTestId('thread-count')).toHaveText('4 replies');
			await expect(root).toHaveAccessibleName('↳ 4 replies');
		}
		await shot(page, `chat-live-${place}-delivered`);
		expect.soft(pendingOffers).toEqual([]);
		expect(deliveredOffers).toHaveLength(1);
		expect(deliveredOffers[0].log).toBe(place === 'thread' ? 'Thread messages' : 'Messages');
		expect(deliveredOffers[0].text).toContain(body);
		expect(deliveredOffers[0].text).not.toMatch(/Sending|replies|Waiting/);
	});
}

test('CHAT-8 BUG-66: the first reply adds an accessible counter without announcing it', async ({ page }) => {
	await open(page);
	const handoff = page.getByTestId('message-list').getByTestId('message').filter({ has: page.getByTestId('handoff-card') });
	await handoff.getByRole('button', { name: 'Reply…', exact: true }).click();
	const pane = page.getByTestId('thread-panel');
	await expect(pane.getByTestId('thread-count')).toHaveText('0 replies');
	await take(page);
	const composer = pane.getByTestId('composer');
	await composer.fill('First delivered handoff reply');
	await composer.press('Control+Enter');
	await expect(pane.getByTestId('message-body').filter({ hasText: 'First delivered handoff reply' })).toBeVisible();
	await expect(handoff.getByRole('button', { name: '↳ 1 reply', exact: true })).toBeVisible();
	await expect(pane.getByTestId('thread-count')).toHaveText('1 reply');
	const offers = await take(page);
	expect(offers).toHaveLength(1);
	expect(offers[0]).toMatchObject({ log: 'Thread messages', text: expect.stringContaining('First delivered handoff reply') });
	await shot(page, 'chat-live-first-reply-readable-counter');
});

test('CHAT-1 BUG-66: the first delivered channel message is offered without the day divider', async ({ page }) => {
	await open(page);
	await page.getByRole('navigation', { name: 'Chat', exact: true }).getByRole('button', { name: 'Add channel', exact: true }).click();
	const dialog = page.getByRole('dialog', { name: 'Add a channel', exact: true });
	await dialog.getByLabel('Name').fill('live-empty');
	await dialog.getByRole('button', { name: 'Add channel', exact: true }).click();
	await expect(page.getByTestId('conversation-title')).toHaveText('#live-empty');
	await expect(page.getByTestId('message-list')).toContainText('No messages yet.');
	await take(page);
	const composer = page.getByTestId('composer');
	await composer.fill('First delivered channel message');
	await composer.press('Control+Enter');
	await expect(page.getByTestId('message-list').getByTestId('message-body')).toHaveText('First delivered channel message');
	const offers = await take(page);
	expect(offers).toHaveLength(1);
	expect(offers[0]).toMatchObject({ log: 'Messages', text: expect.stringContaining('First delivered channel message') });
	expect(offers[0].text).not.toContain('Today');
	await shot(page, 'chat-live-first-channel-message');
});

test('CHAT-5/6 BUG-66: minute ticks and refresh leave readable ask clocks silent', async ({ page }) => {
	await page.clock.install();
	await open(page, 'c-project-atlas');
	const waiting = page.getByTestId('message-list').getByTestId('waiting').first();
	const deadline = page.getByTestId('message-list').getByTestId('deadline').first();
	await expect(waiting).toBeVisible();
	await expect(deadline).toBeVisible();
	const before = await waiting.innerText();
	await take(page);
	await page.clock.runFor(125_000);
	await expect(waiting).not.toHaveText(before);
	await expect(waiting).toMatchAriaSnapshot('- text: /Waiting/');
	await expect(deadline).toMatchAriaSnapshot('- text: /If no answer/');
	expect(await take(page)).toEqual([]);
	await shot(page, 'chat-live-readable-silent-clocks');
});

test('CHAT-1 BUG-66: an added reaction count remains accessible without another message announcement', async ({ page }) => {
	await open(page);
	const message = page.getByTestId('message-list').getByTestId('message').filter({ hasText: 'your part and status for Phase 1' });
	const originalRow = await message.elementHandle();
	const addedRows = await message.evaluateHandle((element) => {
		const ids: string[] = [];
		new MutationObserver((changes) => {
			for (const change of changes) for (const node of change.addedNodes) {
				if (!(node instanceof Element)) continue;
				const rows = [...node.querySelectorAll('[data-testid="message"]')];
				if (node.matches('[data-testid="message"]')) rows.push(node);
				ids.push(...rows.map((row) => row.getAttribute('data-id')!));
			}
		}).observe(element.closest('[role="log"]')!, { childList: true, subtree: true });
		return ids;
	});
	await message.hover();
	await message.getByRole('button', { name: 'React Seen', exact: true }).click();
	await expect(message.getByRole('button', { name: 'Seen, 1', exact: true })).toBeVisible();
	await expect(message.getByRole('button', { name: 'Seen, 1', exact: true })).toHaveAttribute('aria-pressed', 'true');
	expect(await take(page)).toEqual([]);
	expect(await originalRow!.evaluate((element) => element.isConnected)).toBe(true);
	expect(await addedRows.jsonValue()).toEqual([]);
	// Prove the row observer is active, rather than accepting a vacuous empty result.
	await page.getByTestId('composer').fill('Delivered row observer control');
	await page.getByTestId('composer').press('Control+Enter');
	await expect(page.getByRole('log', { name: 'Messages', exact: true })).toContainText('Delivered row observer control');
	expect(await take(page)).toHaveLength(1);
	expect(await addedRows.jsonValue()).toHaveLength(1);
	await shot(page, 'chat-live-readable-silent-reaction');
});

test('CHAT-1/8 BUG-66: switching threads mounts a silent new log', async ({ page }) => {
	await open(page);
	await page.getByTestId('replies-link').first().click();
	const log = page.getByRole('log', { name: 'Thread messages', exact: true });
	await expect(log).toContainText('Storage is done');
	const originalLog = await log.elementHandle();
	await take(page);
	const handoff = page.getByTestId('message-list').getByTestId('handoff-card');
	await handoff.getByRole('button', { name: 'Reply…', exact: true }).click();
	await expect(log).toContainText('Please review the store traits');
	await expect(log).not.toContainText('Storage is done');
	expect(await originalLog!.evaluate((element) => element.isConnected)).toBe(false);
	expect(await take(page)).toEqual([]);
	await shot(page, 'chat-live-silent-thread-switch');
});

test('CHAT-1/7 BUG-66: an incoming open-DM reply is offered once; another conversation stays silent', async ({ page }) => {
	await page.clock.install();
	await open(page, 'c-dm-ines');
	const composer = page.getByTestId('composer');
	await composer.fill('First live DM request');
	await composer.press('Control+Enter');
	await expect(page.getByRole('log', { name: 'Messages', exact: true })).toContainText('First live DM request');
	await take(page);
	await page.clock.runFor(2_000);
	await expect(page.getByTestId('message-list')).toContainText('Sample reply');
	const incoming = await take(page);
	expect(incoming).toHaveLength(1);
	expect(incoming[0]).toMatchObject({ log: 'Messages', text: expect.stringContaining('Sample reply') });
	await composer.fill('Reply after I leave');
	await composer.press('Control+Enter');
	await expect(page.getByRole('log', { name: 'Messages', exact: true })).toContainText('Reply after I leave');
	await page.getByRole('navigation', { name: 'Chat', exact: true }).locator('[data-conversation="c-general"]').click();
	await expect(page.getByTestId('conversation-title')).toHaveText('#general');
	await take(page);
	await page.clock.runFor(2_000);
	expect(await take(page)).toEqual([]);
	await shot(page, 'chat-live-background-dm-silent');
});

test('CHAT-1/13/33 BUG-66: channel, Search and history changes do not offer old messages', async ({ page }) => {
	await open(page);
	await page.getByRole('navigation', { name: 'Chat', exact: true }).locator('[data-conversation="c-general"]').click();
	await expect(page.getByTestId('conversation-title')).toHaveText('#general');
	expect(await take(page)).toEqual([]);
	await page.goBack();
	await expect(page.getByTestId('conversation-title')).toHaveText('#release-v1');
	expect(await take(page)).toEqual([]);
	await page.goForward();
	await expect(page.getByTestId('conversation-title')).toHaveText('#general');
	expect(await take(page)).toEqual([]);
	await page.getByTestId('chat-search').fill('review');
	await expect(page.getByTestId('search-count')).toContainText('results');
	await expect(page.getByTestId('message-list').getByTestId('message').first()).toBeVisible();
	expect(await take(page)).toEqual([]);
	await page.getByTestId('chat-search').fill('');
	await expect(page.getByTestId('conversation-title')).toHaveText('#general');
	expect(await take(page)).toEqual([]);
	await shot(page, 'chat-live-history-search-silent');
});

for (const width of [1440, 960]) {
	test(`CHAT-8 BUG-71: only the open Thread offers its Also-send delivery at ${width}px`, async ({ page }) => {
		await open(page);
		await page.setViewportSize({ width, height: 900 });
		const main = page.getByTestId('message-list');
		await main.getByTestId('replies-link').first().click();
		const pane = page.getByTestId('thread-panel');
		await expect(pane).toBeVisible();
		await expect.poll(() => page.getByRole('region', { name: 'Conversation', exact: true, includeHidden: true }).evaluate((element) => (element as HTMLElement).inert)).toBe(width < 1280);
		expect(await take(page)).toEqual([]);
		await holdSend(page);
		await pane.getByRole('checkbox', { name: 'Also send to #release-v1', exact: true }).check();
		const body = `Also-send offer at ${width}px`;
		await pane.getByTestId('composer').fill(body);
		await pane.getByTestId('composer').press('Control+Enter');
		await expect(pane.getByTestId('sending')).toBeVisible();
		await expect(main.getByTestId('sending')).toHaveCount(1);
		expect(await take(page)).toEqual([]);
		await page.evaluate(() => window.releaseChatSend());
		await expect(pane.getByTestId('sending')).toHaveCount(0);
		await expect(main.getByTestId('sending')).toHaveCount(0);
		const threadCopy = pane.getByTestId('message').filter({ hasText: body });
		const channelCopy = main.getByTestId('message').filter({ hasText: body });
		await expect(threadCopy).toHaveCount(1);
		await expect(channelCopy).toHaveCount(1);
		const id = await threadCopy.getAttribute('data-id');
		expect(id).toBeTruthy();
		expect(id!).not.toMatch(/^pending-/);
		await expect(channelCopy).toHaveAttribute('data-id', id!);
		const offers = await take(page);
		expect.soft(offers).toHaveLength(1);
		expect.soft(offers).toEqual([{ log: 'Thread messages', text: expect.stringContaining(body) }]);
		if (width >= 1280) {
			// live=off must keep the visual channel copy available for manual AT navigation.
			await expect(main.getByRole('article').filter({ hasText: body })).toHaveCount(1);
			await channelCopy.scrollIntoViewIfNeeded();
			await shot(page, 'chat-also-send-both-visible-copies');
			// A filter for all channel rows while any Thread is open would hide this offer.
			const normal = 'A normal channel post with a Thread open';
			const composer = page.getByRole('region', { name: 'Conversation', exact: true }).getByTestId('composer');
			await composer.fill(normal);
			await composer.press('Control+Enter');
			await expect(main.getByTestId('sending')).toBeVisible();
			expect(await take(page)).toEqual([]);
			await page.evaluate(() => window.releaseChatSend());
			await expect(main.getByTestId('sending')).toHaveCount(0);
			await expect(main.getByTestId('message-body').filter({ hasText: normal })).toBeVisible();
			expect(await take(page)).toEqual([{ log: 'Messages', text: expect.stringContaining(normal) }]);
		}
		await pane.getByRole('button', { name: 'Close thread', exact: true }).click();
		await expect(pane).toHaveCount(0);
		await expect(main.getByRole('article').filter({ hasText: body })).toHaveCount(1);
		expect(await take(page)).toEqual([]);
		await shot(page, `chat-also-send-${width}-closed`);
	});
}

for (const context of ['closed', 'another Thread'] as const) {
	test(`CHAT-8 BUG-71: a held Also-send reply reaches the channel with its Thread ${context}`, async ({ page }) => {
		await open(page);
		const main = page.getByTestId('message-list');
		await main.getByTestId('replies-link').first().click();
		const pane = page.getByTestId('thread-panel');
		await expect(pane).toBeVisible();
		await holdSend(page);
		await pane.getByRole('checkbox', { name: 'Also send to #release-v1', exact: true }).check();
		const body = `Deliver after context changed: ${context}`;
		await pane.getByTestId('composer').fill(body);
		await pane.getByTestId('composer').press('Control+Enter');
		await expect(pane.getByTestId('sending')).toBeVisible();
		expect(await take(page)).toEqual([]);
		if (context === 'closed') {
			await pane.getByRole('button', { name: 'Close thread', exact: true }).click();
			await expect(pane).toHaveCount(0);
		} else {
			await main.getByTestId('handoff-card').getByRole('button', { name: 'Reply…', exact: true }).click();
			await expect(pane).toContainText('Please review the store traits');
			await expect(pane.getByTestId('sending')).toHaveCount(0);
		}
		expect(await take(page)).toEqual([]);
		await page.evaluate(() => window.releaseChatSend());
		await expect(main.getByTestId('sending')).toHaveCount(0);
		await expect(main.getByRole('article').filter({ hasText: body })).toHaveCount(1);
		expect(await take(page)).toEqual([{ log: 'Messages', text: expect.stringContaining(body) }]);
		if (context !== 'closed') await expect(pane.getByTestId('message-body').filter({ hasText: body })).toHaveCount(0);
		await shot(page, `chat-also-send-${context.replaceAll(' ', '-')}-delivery`);
	});
}
