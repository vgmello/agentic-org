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
	test(`CHAT-29/30 BUG-67: names and title stay readable at ${pixels}px text and ${width}px viewport`, async ({ page }) => {
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

test('CHAT-29 BUG-67: a long unbroken channel name wraps inside the fixed list', async ({ page }) => {
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
	test(`CHAT-13 BUG-68: Search uses its available width at ${width}px`, async ({ page }) => {
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
