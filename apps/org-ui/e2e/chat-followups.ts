import { expect, type Locator, type Page } from '@playwright/test';
import { create } from './non-chat-icons';
import { visit } from './ready';

export async function openChat(page: Page, width = 1280, scale = 1, url = '/chat') {
	await create(page, width, scale);
	await visit(page, url);
	await expect(page.getByTestId('conversation-title')).toBeAttached();
	await expect(page.getByRole('region', { name: 'Conversation', exact: true })).toBeVisible();
}
export async function navigation(page: Page) {
	const nav = page.getByRole('navigation', { name: 'Chat', exact: true });
	if (!(await nav.isVisible())) await page.getByRole('button', { name: /^Conversations(?:,|$)/ }).click();
	await expect(nav).toBeVisible();
	return nav;
}
export async function inheritsColour(icon: Locator, control: Locator) {
	await expect(icon).toHaveCount(1);
	await expect(icon).toHaveCSS('color', await control.evaluate((e) => getComputedStyle(e).color));
}
export async function keyboardFocus(page: Page, control: Locator) {
	await page.keyboard.press('Tab');
	await control.focus();
	await expect(control).toBeFocused();
}
export async function focusOutline(control: Locator) {
	await expect(control).toBeFocused();
	await expect.poll(() => control.evaluate((e) => {
		const style = getComputedStyle(e);
		return { visible: e.matches(':focus-visible'), width: parseFloat(style.outlineWidth), offset: parseFloat(style.outlineOffset), style: style.outlineStyle, colour: style.outlineColor, expected: getComputedStyle(document.documentElement).getPropertyValue('--color-indigo-400').trim() };
	})).toMatchObject({ visible: true, width: 2, offset: 2, style: 'solid' });
	// Resolve the actual design token through CSS rather than comparing an OKLCH string to RGB.
	await expect.poll(() => control.evaluate((e) => {
		const probe = document.createElement('span'); probe.style.color = 'var(--color-indigo-400)'; e.append(probe);
		const matches = getComputedStyle(e).outlineColor === getComputedStyle(probe).color; probe.remove(); return matches;
	})).toBe(true);
}
export async function sidebarGeometry(nav: Locator) {
	const rows = nav.locator('[data-testid="needs-you-link"], [data-testid="mentions-link"], [data-testid="threads-link"], [data-testid="conversation-link"]');
	expect(await rows.count()).toBeGreaterThan(5);
	for (const row of await rows.all()) {
		if (!(await row.isVisible())) continue;
		const measured = await row.evaluate((e) => {
			const box = e.getBoundingClientRect(), visual = e.firstElementChild!, icon = visual.getBoundingClientRect();
			const content = e.lastElementChild!, label = content.firstElementChild!, range = document.createRange(); range.selectNodeContents(label);
			const lines = [...range.getClientRects()].filter((r) => r.width > 0), first = lines[0];
			const counts = [...content.children].slice(1).map((c) => c.getBoundingClientRect());
			const words = [...label.textContent!.matchAll(/\S+/g)].map((word) => {
				const wordRange = document.createRange(); wordRange.setStart(label.firstChild!, word.index!); wordRange.setEnd(label.firstChild!, word.index! + word[0].length);
				return { word: word[0], fragments: [...wordRange.getClientRects()].filter((r) => r.width > 0).length };
			});
			const root = parseFloat(getComputedStyle(document.documentElement).fontSize);
			return { name: e.textContent, words, fixed: e.getAttribute('data-testid') !== 'conversation-link', lines: lines.length,
				iconBefore: !!first && icon.right <= first.left + 1, sharesLine: !!first && icon.top < first.bottom && icon.bottom > first.top,
				fits: [...lines, ...counts].every((r) => r.left >= icon.right - 1 && r.right <= box.right + 1),
				noOverlap: counts.every((c) => lines.every((r) => c.right <= r.left + .5 || c.left >= r.right - .5 || c.bottom <= r.top + .5 || c.top >= r.bottom - .5)),
				iconSize: visual.tagName.toLowerCase() === 'svg' ? { width: icon.width, height: icon.height, expected: Math.min(root * 1.5, 32) } : undefined };
		});
		expect(measured.lines).toBeGreaterThan(0);
		expect(measured, measured.name ?? '').toMatchObject({ iconBefore: true, sharesLine: true, fits: true, noOverlap: true });
		if (measured.iconSize) {
			expect(measured.iconSize.width).toBeCloseTo(measured.iconSize.expected, 1);
			expect(measured.iconSize.height).toBeCloseTo(measured.iconSize.expected, 1);
		}
		if (measured.fixed) {
			expect(measured.words.length).toBeGreaterThan(0);
			for (const word of measured.words) expect(word.fragments, `${measured.name}: whole word ${word.word}`).toBe(1);
		}
	}
}

export async function kindGeometry(badge: Locator) {
	const geometry = await badge.evaluate((e) => {
		const icon = e.querySelector('svg')!.getBoundingClientRect(), walker = document.createTreeWalker(e, NodeFilter.SHOW_TEXT);
		let word: DOMRect | undefined;
		for (let n = walker.nextNode(); n; n = walker.nextNode()) if (n.textContent?.includes('Announcement')) { const range = document.createRange(); range.selectNode(n); word = range.getBoundingClientRect(); break; }
		const unit = e.querySelector('span')!.getBoundingClientRect(), column = e.parentElement!, available = column.getBoundingClientRect();
		return { wordFound: !!word, beside: !!word && icon.right <= word.left + 1 && icon.top < word.bottom && icon.bottom > word.top, centred: !!word && Math.abs((icon.top + icon.bottom) / 2 - (word.top + word.bottom) / 2) <= 2, fits: unit.left >= available.left - 1 && unit.right <= Math.min(available.right, innerWidth) + .5 && column.scrollWidth <= column.clientWidth + .5, measured: { left: unit.left, right: unit.right, width: unit.width, availableRight: available.right, viewport: innerWidth, clientWidth: column.clientWidth, scrollWidth: column.scrollWidth } };
	});
	expect(geometry, JSON.stringify(geometry)).toMatchObject({ wordFound: true, beside: true, centred: true, fits: true });
}
export async function attachmentGeometry(chip: Locator, wraps = false) {
	const geometry = await chip.evaluate((e) => {
		const icon = e.querySelector('svg')!.getBoundingClientRect(), range = document.createRange(); range.selectNodeContents(e.querySelector('span')!);
		const lines = [...range.getClientRects()];
		const span = e.querySelector('span')!, spanBox = span.getBoundingClientRect(), lineHeight = parseFloat(getComputedStyle(span).lineHeight);
		return { lineBoxOffset: Math.abs(icon.top - (spanBox.top + (lineHeight - icon.height) / 2)), lines: lines.length, firstLineOffset: Math.abs((icon.top + icon.bottom) / 2 - (lines[0].top + lines[0].bottom) / 2), scale: parseFloat(getComputedStyle(document.documentElement).fontSize) / 16 };
	});
	expect(geometry.lines).toBeGreaterThan(wraps ? 1 : 0);
	expect(geometry.firstLineOffset).toBeLessThanOrEqual(4 * geometry.scale);
	expect(geometry.lineBoxOffset).toBeLessThanOrEqual(.5);
}
