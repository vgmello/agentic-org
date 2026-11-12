import { expect, test, type Locator, type Page } from '@playwright/test';
import { visit } from './ready';
import { shot } from './shot';

const nav = (page: Page) => page.getByRole('navigation', { name: 'Chat', exact: true });
const toggle = (page: Page) => page.getByRole('button', { name: /^Conversations(?:,|$)/ });

async function create(page: Page) {
	await visit(page, '/create');
	await page.getByLabel('Organization name').fill('Acme');
	await page.getByRole('button', { name: 'Create organization' }).click();
	await expect(page).toHaveURL(/\/agents$/);
}

async function open(page: Page, place = '') {
	await create(page);
	await visit(page, `/chat${place}`);
	await expect(page.getByTestId('conversation-title')).toHaveText(place ? '#release-v1' : 'Needs you');
	await expect(nav(page)).toBeVisible();
}

async function fontSize(page: Page, pixels: number) {
	await page.locator('html').evaluate((html, value) => { html.style.fontSize = `${value}px`; }, pixels);
}

// DOM text can remain intact behind ellipsis. Check the actual glyph rectangles
// against the label's box and the whole button, and also inspect scroll extents.
async function readable(label: Locator) {
	await label.scrollIntoViewIfNeeded();
	return label.evaluate((element) => {
		const box = element.getBoundingClientRect();
		const button = element.closest('button')!.getBoundingClientRect();
		const range = document.createRange();
		range.selectNodeContents(element);
		const glyphs = [...range.getClientRects()].filter((r) => r.width > 0 && r.height > 0);
		return {
			glyphs: glyphs.length,
			overflow: element.scrollWidth - element.clientWidth,
			clipped: glyphs.some((r) => r.left < Math.max(box.left, button.left) - 1 || r.right > Math.min(box.right, button.right) + 1 || r.top < box.top - 1 || r.bottom > box.bottom + 1)
		};
	});
}

async function noSidewaysScroll(page: Page, checkNav = true) {
	await expect.poll(() => page.getByRole('main').evaluate((main) => ({
		page: document.documentElement.scrollWidth - innerWidth,
		main: main.scrollWidth - main.clientWidth
	}))).toEqual({ page: 0, main: 0 });
	if (checkNav) await expect.poll(() => nav(page).evaluate((element) => element.scrollWidth - element.clientWidth)).toBeLessThanOrEqual(1);
}

for (const [width, pixels, inline] of [[1440, 32, true], [1279, 32, false], [1024, 32, false], [1440, 20, true]] as const) {
	test(`CHAT-29/30 BUG-67: names and title stay readable at ${pixels}px text and ${width}px viewport`, { tag: '@webkit' }, async ({ page }) => {
		await open(page);
		await page.setViewportSize({ width, height: 900 });
		await fontSize(page, pixels);
		const dialog = page.getByRole('dialog', { name: 'Conversations', exact: true });
		if (!inline) {
			await expect(toggle(page)).toBeVisible();
			await page.keyboard.press('Tab');
			await toggle(page).focus();
			await toggle(page).press('Enter');
			await expect(dialog).toBeVisible();
			await expect(nav(page).getByTestId('needs-you-link')).toBeFocused();
			const close = await dialog.getByRole('button', { name: 'Close', exact: true }).elementHandle();
			const title = dialog.getByRole('heading', { name: 'Conversations', exact: true });
			const geometry = await title.evaluate((element, close) => {
				const a = element.getBoundingClientRect();
				const b = close!.getBoundingClientRect();
				const range = document.createRange();
				range.selectNodeContents(element);
				const glyphs = [...range.getClientRects()].filter((r) => r.width > 0);
				return { glyphs: glyphs.length, clipped: glyphs.some((r) => r.left < a.left - 1 || r.right > a.right + 1), overlaps: glyphs.some((r) => r.left < b.right && r.right > b.left && r.top < b.bottom && r.bottom > b.top) };
			}, close);
			expect(geometry.glyphs).toBeGreaterThan(0);
			expect.soft(geometry.clipped, 'Conversations title glyphs fit their box').toBe(false);
			expect.soft(geometry.overlaps, 'Conversations title does not run under Close').toBe(false);
			expect((await dialog.boundingBox())!.width).toBeCloseTo(256, 0);
		} else {
			await expect(toggle(page)).toHaveCount(0);
			await expect(nav(page)).toBeVisible();
			expect((await nav(page).locator('..').boundingBox())!.width).toBeCloseTo(256, 0);
		}
		await expect(nav(page).locator('[data-conversation="c-release-v1"]')).toHaveAccessibleName('release-v1 6 unread');
		await expect(nav(page).locator('[data-conversation="c-dm-ines"]')).toHaveAccessibleName('Inês Duarte, Asleep');
		for (const name of ['engineering', 'onboarding-kit', 'Inês Duarte', 'Sam Okafor, Noor Haddad']) {
			const scope = name === 'Sam Okafor, Noor Haddad' ? nav(page).locator('[data-conversation="c-group"]') : nav(page);
			const label = scope.getByText(name, { exact: true });
			await expect(label).toHaveCount(1);
			const geometry = await readable(label);
			expect(geometry.glyphs, name).toBeGreaterThan(0);
			expect.soft(geometry.overflow, `${name} scroll extent`).toBeLessThanOrEqual(1);
			expect.soft(geometry.clipped, `${name} glyphs fit their label/button`).toBe(false);
		}
		await noSidewaysScroll(page);
		await shot(page, `chat-layout-${width}-${pixels}-names`);
		if (!inline) {
			for (let i = 0; i < 8; i++) {
				await page.keyboard.press('Tab');
				await expect(dialog.locator(':focus')).toHaveCount(1);
			}
			await page.keyboard.press('Escape');
			await expect(dialog).toBeHidden();
			await expect(toggle(page)).toBeFocused();
		} else {
			await page.keyboard.press('Tab');
			await nav(page).getByTestId('needs-you-link').focus();
			await expect(nav(page).getByTestId('needs-you-link')).toBeFocused();
		}
	});
}

test('CHAT-29 BUG-67: a long unbroken channel name wraps inside the fixed list', { tag: '@webkit' }, async ({ page }) => {
	await create(page);
	const longName = 'a'.repeat(64);
	await page.evaluate(async (name) => {
		const path = '/src/lib/modules/messaging/legacy-api.ts';
		const { chatApi } = await import(path);
		const snapshot = chatApi.snapshot.bind(chatApi);
		chatApi.snapshot = async (id: string) => {
			const result = await snapshot(id);
			return { ...result, state: { ...result.state, conversations: result.state.conversations.map((c: { id: string }) => c.id === 'c-release-v1' ? { ...c, name } : c) } };
		};
	}, longName);
	// SPA navigation preserves the test-owned snapshot override; do it while Main is visible.
	await page.getByRole('navigation', { name: 'Main', exact: true }).getByRole('link', { name: 'Chat', exact: true }).click();
	await expect(page.getByTestId('needs-you-list')).toBeVisible();
	await fontSize(page, 32);
	const row = nav(page).locator('[data-conversation="c-release-v1"]');
	await expect(row).toHaveAccessibleName(`${longName} 6 unread`);
	const geometry = await readable(row.getByText(longName, { exact: true }));
	expect(geometry.glyphs).toBeGreaterThan(0);
	expect(geometry.overflow).toBeLessThanOrEqual(1);
	expect(geometry.clipped).toBe(false);
	await noSidewaysScroll(page);
	await shot(page, 'chat-layout-long-unbroken-name');
});

for (const width of [320, 600, 639, 640, 1440]) {
	test(`CHAT-13 BUG-68: Search uses its available width at ${width}px`, { tag: '@webkit' }, async ({ page }) => {
		await open(page, '?c=c-release-v1');
		await page.setViewportSize({ width, height: 900 });
		if (width <= 640) await expect(toggle(page)).toBeVisible();
		const search = page.getByRole('searchbox', { name: 'Search chat', exact: true });
		const geometry = await search.evaluate((input) => {
			const wrapper = input.parentElement!;
			const row = wrapper.parentElement!;
			const header = row.parentElement!;
			const style = getComputedStyle(header);
			return { available: header.clientWidth - parseFloat(style.paddingLeft) - parseFloat(style.paddingRight), row: row.getBoundingClientRect().width, wrapper: wrapper.getBoundingClientRect().width, input: input.getBoundingClientRect().width };
		});
		if (width < 640) {
			expect.soft(Math.abs(geometry.row - geometry.available), 'full search row').toBeLessThanOrEqual(1);
			expect.soft(Math.abs(geometry.wrapper - geometry.available), 'full search wrapper').toBeLessThanOrEqual(1);
			expect.soft(Math.abs(geometry.input - geometry.available), 'full search input').toBeLessThanOrEqual(1);
		} else {
			expect(geometry.input).toBeCloseTo(224, 0);
		}
		await noSidewaysScroll(page, false);
		await page.keyboard.press('Tab');
		await search.focus();
		await expect(search).toBeFocused();
		expect(await search.evaluate((element) => getComputedStyle(element).outlineStyle)).not.toBe('none');
		await shot(page, `chat-layout-search-${width}`);
		await search.fill('review');
		await expect(page.getByTestId('conversation-title')).toHaveText('Search');
		await expect(page).toHaveURL(/[?&]q=review(?:&|$)/);
		await expect(page.getByTestId('search-count')).toContainText('result');
		await page.goBack();
		await expect(page.getByTestId('conversation-title')).toHaveText('#release-v1');
		await expect(search).toHaveValue('');
	});
}

const previewNotice = 'Previewing how a bridge (Slack, Teams or Discord) shows this: read-only here, and asks to the owner and approvals open in Agentic Org.';
const offlineNotice = "Read-only from the last copy. The office isn't running.";

for (const width of [320, 640, 960, 1280]) for (const scale of [1, 2]) {
	test(`CHAT-1/29 BUG-70: quiet context notices preserve all flag combinations at ${width}px and ${scale * 100}% text`, { tag: '@webkit' }, async ({ page }) => {
		await create(page);
		await page.setViewportSize({ width, height: 900 });
		expect((await page.request.get('/e2e/fakes/org-api.ts')).status()).toBe(200);
		for (const [bridge, offline] of [[false, false], [true, false], [false, true], [true, true]]) {
			const params = new URLSearchParams({ c: 'c-release-v1' });
			if (bridge) params.set('surface', 'bridge');
			if (offline) params.set('chat', 'offline');
			await visit(page, `/chat?${params}`);
			await fontSize(page, 16 * scale);
			await expect(page.getByTestId('conversation-title')).toHaveText('#release-v1');
			const conversation = page.getByRole('region', { name: 'Conversation', exact: true });
			const area = page.getByRole('region', { name: 'Message area', exact: true });
			await expect(area.getByRole('log', { name: 'Messages', exact: true })).toBeVisible();
			await expect(page.getByTestId('bridge-preview')).toHaveCount(bridge ? 1 : 0);
			await expect(page.getByTestId('office-down')).toHaveCount(offline ? 1 : 0);
			for (const [id, shown, text] of [['bridge-preview', bridge, previewNotice], ['office-down', offline, offlineNotice]] as const) {
				if (!shown) continue;
				const notice = page.getByTestId(id);
				await expect(notice).toHaveText(text);
				await expect(notice.getByRole('paragraph')).toHaveText(text);
				expect(await notice.evaluate((element) => !!element.closest('[role=alert], [role=status], [aria-live]'))).toBe(false);
				const style = await notice.evaluate((element) => {
					const s = getComputedStyle(element);
					const canvas = document.createElement('canvas');
					canvas.width = canvas.height = 1;
					const context = canvas.getContext('2d')!;
					context.fillStyle = s.backgroundColor;
					context.fillRect(0, 0, 1, 1);
					const range = document.createRange();
					range.selectNodeContents(element);
					const bounds = element.getBoundingClientRect();
					const glyphs = [...range.getClientRects()].filter((r) => r.width > 0 && r.height > 0);
					return { background: [...context.getImageData(0, 0, 1, 1).data], top: parseFloat(s.borderTopWidth), bottom: parseFloat(s.borderBottomWidth), padding: parseFloat(s.paddingTop), textFits: glyphs.length > 0 && glyphs.every((r) => r.left >= bounds.left - 1 && r.right <= bounds.right + 1) };
				});
				expect.soft(style.background).toEqual([255, 255, 255, 8]);
				expect.soft(style.top).toBe(1);
				expect.soft(style.bottom).toBe(1);
				expect.soft(style.padding).toBe(12 * scale);
				expect.soft(style.textFits).toBe(true);
				expect.soft(await notice.locator('svg').count()).toBe(1);
				expect.soft(await notice.locator('svg').evaluateAll((icons) => icons.every((icon) => !!icon.closest('[aria-hidden=true]')))).toBe(true);
				await shot(page, `chat-context-${width}-${scale}-${bridge}-${offline}-${id}`, { context: notice });
			}
			if (bridge && offline) {
				expect(await page.getByTestId('bridge-preview').evaluate((a) => !!(a.compareDocumentPosition(document.querySelector('[data-testid=office-down]')!) & Node.DOCUMENT_POSITION_FOLLOWING))).toBe(true);
				const paths = (icons: Element[]) => icons.map((icon) => icon.innerHTML);
				expect(await page.getByTestId('bridge-preview').locator('svg').evaluateAll(paths)).not.toEqual(await page.getByTestId('office-down').locator('svg').evaluateAll(paths));
			}
			await expect(conversation.getByTestId('composer')).toHaveCount(bridge || offline ? 0 : 1);
			if (!bridge && !offline) {
				await conversation.getByTestId('composer').focus();
				await expect(conversation.getByTestId('composer')).toBeFocused();
			}
			await area.focus();
			await expect(area).toBeFocused();
			await expect(area).toHaveAccessibleName('Message area');
			await expect(area.getByRole('log', { name: 'Messages', exact: true })).toHaveAttribute('aria-live', 'polite');
			await expect(area.getByRole('log', { name: 'Messages', exact: true })).toHaveAttribute('aria-relevant', 'additions');
			await noSidewaysScroll(page, false);
		}
	});
}
