import { expect, test } from '@playwright/test';
import { visit } from './ready';
import { shot } from './shot';
import { buttons, menu, consumerFocus, keyboardFocus, outline, token, setupFocus, readable, outlineContrast } from './focus-primitives';
for (const width of [320, 640, 960, 1280]) for (const scale of [1, 2]) {
	test(`BUG-101 BUG-70: menu keyboard and pointer contract at ${width}/${scale * 100}`, { tag: '@webkit' }, async ({ page }) => { await menu(page, width, scale); });
	test(`BUG-102 BUG-70: Button six variants and native actions at ${width}/${scale * 100}`, { tag: '@webkit' }, async ({ page }) => { await buttons(page, width, scale); });
	test(`BUG-101/102 BUG-70: nine consumer families at ${width}/${scale * 100}`, { tag: '@webkit' }, async ({ page }) => { await consumerFocus(page, width, scale); });
}

// WebKit has no exposed default-font preference in Playwright. This proves 40px-root rendering,
// while the dedicated Chromium project separately proves a real 20px browser default at 200%.
for (const width of [320, 640, 960, 1280]) {
	test(`BUG-101/102 BUG-70: explicit 40px root matrix at ${width}`, { tag: '@webkit' }, async ({ page }) => { await buttons(page, width, 2.5); });
	test(`BUG-101 BUG-70: explicit 40px root menu at ${width}`, { tag: '@webkit' }, async ({ page }) => { await menu(page, width, 2.5); });
	test(`BUG-102 BUG-70: explicit 40px root consumers at ${width}`, { tag: '@webkit' }, async ({ page }) => { await consumerFocus(page, width, 2.5); });
}

test('BUG-102 BUG-70: ProcessSheet caller override uses the same indigo focus and closes', { tag: '@webkit' }, async ({ page }) => {
	await visit(page, '/create'); await page.getByLabel('Organization name').fill('Acme');
	await page.getByRole('button', { name: 'Create organization' }).click(); await expect(page).toHaveURL(/\/agents$/);
	await visit(page, '/processes?place=team:web'); await page.locator('[data-testid=process-row][data-key="code-review"]').click();
	const close = page.getByRole('dialog').getByRole('button', { name: 'Close', exact: true });
	await keyboardFocus(page, close); await outline(close, 2, await token(page));
	await shot(page, 'focus-processes-caller-override', { context: close });
	await close.press('Enter'); await expect(page.getByRole('dialog')).toHaveCount(0);
});

test('BUG-101/102 BUG-70: outline and readable-text guards allow real controls and catch collapsed or weak ones', { tag: '@webkit' }, async ({ page }) => {
	const fixture = await setupFocus(page, 320, 2);
	const control = fixture.getByRole('group', { name: 'default', exact: true }).getByRole('button');
	await expect(control).toHaveCount(1);
	await keyboardFocus(page, control); await outline(control, 2, await token(page)); await readable(control);
	const font = await control.evaluate((element) => getComputedStyle(element).fontSize);
	await control.evaluate((element) => { element.style.fontSize = '0px'; });
	await expect(control).toHaveCSS('font-size', '0px');
	await expect(readable(control)).rejects.toThrow();
	await control.evaluate((element) => { element.style.fontSize = ''; element.style.width = '1px'; element.style.padding = '0'; });
	await expect(control).toHaveCSS('font-size', font);
	await control.evaluate((element) => Promise.all(element.getAnimations().map((animation) => animation.finished.catch(() => undefined))));
	await expect(readable(control)).rejects.toThrow();
	await control.evaluate((element) => { element.style.width = ''; element.style.padding = ''; element.textContent = 'A'; });
	await control.evaluate((element) => Promise.all(element.getAnimations().map((animation) => animation.finished.catch(() => undefined))));
	await expect(readable(control)).rejects.toThrow();
	await control.evaluate((element) => { element.textContent = 'Activate'; element.style.outlineColor = 'var(--color-gray-600)'; });
	await expect(control).toHaveCSS('outline-color', await token(page, '--color-gray-600'));
	expect(await outlineContrast(control, false)).toBeLessThan(3);
	await control.evaluate((element) => { element.style.outlineColor = ''; });
	await outline(control, 2, await token(page)); await readable(control);
});

test('BUG-101 BUG-70: keyboard-to-pointer handoff keeps the pinned menu indication', { tag: '@webkit' }, async ({ page }) => {
	const fixture = await setupFocus(page, 320, 1);
	const trigger = fixture.getByRole('button', { name: 'Commands', exact: true });
	await keyboardFocus(page, trigger); await trigger.press('ArrowDown');
	await outline(page.getByRole('menuitem', { name: 'Rename item', exact: true }), -2, await token(page));
	const target = page.getByRole('menuitem', { name: 'Delete item', exact: true });
	await target.hover(); await expect(target).toBeFocused();
	console.info('Mixed-input native state:', await target.evaluate((element) => ({ visible: element.matches(':focus-visible'), style: getComputedStyle(element).outlineStyle, width: getComputedStyle(element).outlineWidth })));
	await shot(page, 'focus-keyboard-to-pointer', { context: page.getByRole('menu') });
	// Ada01M77BPB: preserve native focus-visible through hover and any subsequent pointer handoff.
	await outline(target, -2, await token(page));
	await target.click(); await expect(fixture.getByLabel('Selection')).toHaveText('Deleted');
	await expect(page.getByRole('menu')).toHaveCount(0);
	await trigger.click(); await target.hover(); await expect(target).toBeFocused();
	// Chromium retains the native indication here; WebKit clears it. The browser owns this state.
	const nativeVisible = await target.evaluate((element) => element.matches(':focus-visible'));
	console.info('Pointer reopen native focus-visible:', nativeVisible);
	if (nativeVisible) await outline(target, -2, await token(page));
	else await expect(target).toHaveCSS('outline-style', 'none');
	await shot(page, 'focus-pointer-reopen-native', { context: page.getByRole('menu') });
	await page.keyboard.press('Escape'); await expect(page.getByRole('menu')).toHaveCount(0);
	await fixture.getByTestId('focus-start').click(); await trigger.click(); await target.hover();
	await expect(target).toBeFocused();
	expect(await target.evaluate((element) => element.matches(':focus-visible'))).toBe(false);
	await expect(target).toHaveCSS('outline-style', 'none');
	await shot(page, 'focus-fresh-pointer-reset', { context: page.getByRole('menu') });
	await page.keyboard.press('Escape'); await expect(page.getByRole('menu')).toHaveCount(0);
});
