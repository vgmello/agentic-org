import { expect, type Locator, type Page } from '@playwright/test';
import { visit } from './ready';
import { shot } from './shot';

export async function setupFocus(page: Page, width: number, scale: number, root = 16) {
	await page.setViewportSize({ width, height: 1100 });
	await visit(page, '/create');
	await page.getByLabel('Organization name').fill('Acme');
	await page.getByRole('button', { name: 'Create organization' }).click();
	await expect(page).toHaveURL(/\/agents$/);
	// Percentages retain the actual browser default; the dedicated project must prove a 20px default.
	await page.locator('html').evaluate((html, percent) => { html.style.fontSize = `${percent}%`; }, scale * 100);
	await expect(page.locator('html')).toHaveCSS('font-size', `${root * scale}px`);
	await page.evaluate(async () => {
		const path = '/src/lib/components/testing/focus-primitives.ts';
		const { mountFocusPrimitives } = await import(path);
		const target = document.createElement('div'); document.body.append(target); mountFocusPrimitives(target);
	});
	await page.evaluate(() => document.fonts.ready.then(() => undefined));
	return page.getByRole('region', { name: 'Focus contracts', exact: true });
}

export async function token(page: Page, name = '--color-indigo-400') {
	return page.evaluate((name) => {
		const node = document.createElement('span'); node.style.color = `var(${name})`; document.body.append(node);
		const color = getComputedStyle(node).color; node.remove(); return color;
	}, name);
}
export async function outline(control: Locator, offset = 2, color?: string) {
	await expect(control).toBeFocused();
	await expect(control).toHaveCSS('outline-style', 'solid');
	await expect(control).toHaveCSS('outline-width', '2px');
	await expect(control).toHaveCSS('outline-offset', `${offset}px`);
	if (color) await expect(control).toHaveCSS('outline-color', color);
	expect(await outlineContrast(control, offset < 0)).toBeGreaterThanOrEqual(3);
}
export async function outlineContrast(control: Locator, inset: boolean) {
	return control.evaluate((element, inset) => {
		const canvas = document.createElement('canvas'); canvas.width = canvas.height = 1;
		const ctx = canvas.getContext('2d')!;
		const rgba = (color: string) => { ctx.clearRect(0, 0, 1, 1); ctx.fillStyle = color; ctx.fillRect(0, 0, 1, 1); return [...ctx.getImageData(0, 0, 1, 1).data]; };
		const colors = [];
		for (let node: Element | null = inset ? element : element.parentElement; node; node = node.parentElement) colors.push(rgba(getComputedStyle(node).backgroundColor));
		const background = colors.reverse().reduce((back, front) => back.map((c, i) => front[i] * front[3] / 255 + c * (1 - front[3] / 255)), [255, 255, 255]);
		const luminance = (color: number[]) => color.slice(0, 3).map((c) => c / 255).map((c) => c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4).reduce((sum, c, i) => sum + c * [0.2126, 0.7152, 0.0722][i], 0);
		const a = luminance(rgba(getComputedStyle(element).outlineColor)), b = luminance(background);
		return (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05);
	}, inset);
}
export async function keyboardFocus(page: Page, control: Locator) {
	await page.keyboard.press('Tab'); await control.focus(); // Real keyboard modality, including native WebKit.
}
export async function readable(control: Locator) {
	const size = await control.evaluate((element) => {
		const range = document.createRange(); range.selectNodeContents(element);
		const text = range.getBoundingClientRect(), box = element.getBoundingClientRect();
		const canvas = document.createElement('canvas'), context = canvas.getContext('2d')!;
		const style = getComputedStyle(element); context.font = `${style.fontWeight} ${style.fontSize} ${style.fontFamily}`;
		return { width: text.width, minimum: context.measureText('000000').width, fits: text.left >= box.left - 1 && text.right <= box.right + 1, viewport: box.left >= 0 && box.right <= innerWidth };
	});
	expect(size.width).toBeGreaterThan(0); expect(size.width).toBeGreaterThanOrEqual(size.minimum);
	expect(size.fits).toBe(true); expect(size.viewport).toBe(true);
}
export async function buttons(page: Page, width: number, scale: number, root = 16) {
	const fixture = await setupFocus(page, width, scale, root), indigo = await token(page);
	let count = 0;
	for (const variant of ['default', 'outline', 'secondary', 'ghost', 'destructive', 'link']) {
		const group = fixture.getByRole('group', { name: variant, exact: true });
		for (const name of ['Activate', 'Navigate']) {
			const control = group.getByRole(name === 'Activate' ? 'button' : 'link', { name, exact: true });
			const border = await control.evaluate((element) => getComputedStyle(element).borderTopColor);
			await keyboardFocus(page, control); await outline(control, 2, indigo); await readable(control);
			await expect(control).toHaveCSS('border-top-color', border);
			if (name === 'Activate') {
				await expect.poll(() => control.evaluate((element) => getComputedStyle(element).getPropertyValue('--tw-ring-shadow'))).toBe('0 0 #0000');
				await control.press('Enter'); await expect(fixture.getByLabel('Activations')).toHaveText(String(++count));
				await control.press('Space'); await expect(fixture.getByLabel('Activations')).toHaveText(String(++count));
			} else {
				await control.press('Enter'); await expect(page).toHaveURL(/#focus-link$/);
			}
			await shot(page, `focus-${variant}-${name}-${width}-${root * scale}`, { context: control });
			await fixture.getByTestId('focus-start').click(); await control.click();
			await expect.poll(() => control.evaluate((element) => element.matches(':focus-visible'))).toBe(false);
			await expect(control).toHaveCSS('outline-style', 'none');
			if (name === 'Activate') await expect(fixture.getByLabel('Activations')).toHaveText(String(++count));
		}
	}
	const invalid = fixture.getByRole('button', { name: 'Invalid', exact: true });
	await keyboardFocus(page, invalid); await outline(invalid, 2, indigo);
	await expect.poll(() => pixel(invalid, 'border-top-color')).toEqual(await colorPixel(page, 'color-mix(in oklab, var(--color-destructive) 50%, transparent)'));
	await expect.poll(() => pixel(invalid, '--tw-ring-color')).toEqual(await colorPixel(page, 'color-mix(in oklab, var(--color-destructive) 40%, transparent)'));
	await expect.poll(() => invalid.evaluate((element) => getComputedStyle(element).getPropertyValue('--tw-ring-shadow'))).not.toBe('0 0 #0000');
	const override = fixture.getByRole('button', { name: 'Override', exact: true });
	await keyboardFocus(page, override); await outline(override, 2, await token(page, '--color-emerald-400'));
	await expect(fixture.getByRole('button', { name: 'Disabled', exact: true })).toBeDisabled();
	const disabledLink = fixture.getByRole('link', { name: 'Disabled link', exact: true });
	await expect(disabledLink).toHaveAttribute('aria-disabled', 'true'); await expect(disabledLink).toHaveAttribute('tabindex', '-1');
	await expect(disabledLink).not.toHaveAttribute('href');
	await fixture.getByRole('group', { name: 'link', exact: true }).getByRole('link').focus();
	await page.keyboard.press(page.context().browser()?.browserType().name() === 'webkit' && process.platform === 'darwin' ? 'Alt+Tab' : 'Tab'); await expect(invalid).toBeFocused();
	await shot(page, `focus-invalid-${width}-${root * scale}`, { context: invalid });
}

export async function menu(page: Page, width: number, scale: number, root = 16) {
	const fixture = await setupFocus(page, width, scale, root), indigo = await token(page);
	const trigger = fixture.getByRole('button', { name: 'Commands', exact: true });
	await keyboardFocus(page, trigger); await trigger.press('ArrowDown');
	const normal = page.getByRole('menuitem', { name: 'Rename item', exact: true });
	const blocked = page.getByRole('menuitem', { name: 'Blocked item', exact: true });
	const destructive = page.getByRole('menuitem', { name: 'Delete item', exact: true });
	await expect(page.getByRole('menuitem', { name: 'Native disabled', exact: true })).toHaveAttribute('aria-disabled', 'true');
	await expect(page.getByRole('menuitem', { name: 'Native disabled', exact: true })).toHaveCSS('pointer-events', 'none');
	await outline(normal, -2, indigo); await readable(normal);
	await expect(normal).toHaveCSS('color', await token(page, '--color-accent-foreground'));
	await shot(page, `focus-menu-normal-${width}-${root * scale}`, { context: page.getByRole('menu') });
	await normal.press('ArrowDown'); await outline(blocked, -2, indigo); await expect(blocked).toHaveAttribute('aria-disabled', 'true');
	await blocked.press('Enter'); await expect(page.getByRole('menu')).toBeVisible(); await expect(fixture.getByLabel('Selection')).toHaveText('Nothing selected');
	await blocked.press('End'); await outline(destructive, -2, indigo); await readable(destructive);
	await expect(destructive).toHaveCSS('color', await token(page, '--color-destructive'));
	await shot(page, `focus-menu-destructive-${width}-${root * scale}`, { context: page.getByRole('menu') });
	await destructive.press('Home'); await expect(normal).toBeFocused();
	await normal.press('d'); await expect(destructive).toBeFocused();
	await destructive.press('Escape'); await expect(page.getByRole('menu')).toHaveCount(0); await expect(trigger).toBeFocused();
	await fixture.getByTestId('focus-start').click(); await trigger.click(); await normal.hover();
	await expect(normal).toHaveCSS('outline-style', 'none');
	await expect(normal).not.toHaveCSS('outline-color', indigo);
	await destructive.hover(); await expect(destructive).toHaveCSS('outline-style', 'none');
	await hoverTint(destructive);
	await normal.hover();
	await hoverTint(normal);
	await shot(page, `focus-menu-pointer-${width}-${root * scale}`, { context: page.getByRole('menu') });
	await normal.click(); await expect(fixture.getByLabel('Selection')).toHaveText('Renamed'); await expect(page.getByRole('menu')).toHaveCount(0);
	await trigger.click(); await page.mouse.click(0, 0); await expect(page.getByRole('menu')).toHaveCount(0);
	await keyboardFocus(page, trigger); await trigger.press('ArrowDown'); await normal.press('End'); await destructive.press('Enter');
	await expect(fixture.getByLabel('Selection')).toHaveText('Deleted'); await expect(page.getByRole('menu')).toHaveCount(0); await expect(trigger).toBeFocused();
}

export async function pixel(control: Locator, property: string) {
	return control.evaluate((element, property) => {
		const canvas = document.createElement('canvas'); canvas.width = canvas.height = 1;
		const context = canvas.getContext('2d')!; context.fillStyle = getComputedStyle(element).getPropertyValue(property); context.fillRect(0, 0, 1, 1);
		return [...context.getImageData(0, 0, 1, 1).data];
	}, property);
}
export async function colorPixel(page: Page, color: string) {
	return page.evaluate((color) => {
		const node = document.createElement('span'); node.style.color = color; document.body.append(node);
		const canvas = document.createElement('canvas'); canvas.width = canvas.height = 1;
		const context = canvas.getContext('2d')!; context.fillStyle = getComputedStyle(node).color; context.fillRect(0, 0, 1, 1); node.remove();
		return [...context.getImageData(0, 0, 1, 1).data];
	}, color);
}
export async function hoverTint(control: Locator) {
	await expect.poll(() => pixel(control, 'background-color')).toEqual([255, 255, 255, 13]); // 5% white, independent of engine CSS colour serialization.
}

export async function consumerFocus(page: Page, width: number, scale: number, root = 16) {
	await page.setViewportSize({ width, height: 1100 });
	const resize = async () => {
		await page.locator('html').evaluate((html, percent) => { html.style.fontSize = `${percent}%`; }, scale * 100);
		await expect(page.locator('html')).toHaveCSS('font-size', `${root * scale}px`);
	};
	const probe = async (control: Locator, family: string) => {
		await keyboardFocus(page, control); await outline(control, 2, await token(page));
		await shot(page, `focus-consumer-${family}-${width}-${root * scale}`, { context: control });
	};
	const go = async (path: string) => { await visit(page, path); await resize(); };
	await go('/create');
	await probe(page.getByRole('button', { name: 'Create organization', exact: true }), 'onboarding');
	await page.getByLabel('Organization name').fill('Acme');
	await page.getByRole('button', { name: 'Create organization' }).press('Enter'); await expect(page).toHaveURL(/\/agents$/);
	const add = page.getByRole('main').getByRole('button', { name: 'Add agent', exact: true });
	await probe(add, 'agents'); await add.press('Enter');
	const cancel = page.getByRole('dialog').getByRole('button', { name: 'Cancel', exact: true });
	await probe(cancel, 'sheet-footer'); await cancel.press('Enter'); await expect(page.getByRole('dialog')).toHaveCount(0); await expect(add).toBeFocused();
	await go('/org-chart');
	const view = page.getByRole('button', { name: 'View', exact: true });
	await probe(view, 'org-chart'); await view.press('ArrowDown');
	const item = page.getByRole('menuitem').first(); await outline(item, -2, await token(page));
	await shot(page, `focus-consumer-chart-menu-${width}-${root * scale}`, { context: page.getByRole('menu') });
	await item.press('Escape'); await expect(view).toBeFocused();
	await go('/projects');
	const assign = page.getByRole('button', { name: 'Assign a team to Scratchpad', exact: true });
	await probe(assign, 'projects'); await assign.press('Enter');
	await probe(page.getByRole('dialog').getByRole('button', { name: 'Cancel', exact: true }), 'dialog-footer');
	await page.keyboard.press('Escape'); await expect(assign).toBeFocused();
	await go('/work'); await page.getByRole('tab', { name: /^Requests/ }).click();
	await probe(page.getByTestId('new-request'), 'work-requests');
	await go('/chat?c=c-general'); await probe(page.getByRole('button', { name: 'Members', exact: true }), 'chat');
	await go('/offices'); await page.getByTestId('office-row').first().getByRole('button').click();
	const officeClose = page.getByRole('dialog').getByRole('button', { name: 'Close', exact: true });
	await probe(officeClose, 'offices'); await officeClose.press('Enter'); await expect(page.getByRole('dialog')).toHaveCount(0);
	await go('/processes?place=team:web'); await page.locator('[data-testid=process-row][data-key="code-review"]').click();
	const close = page.getByRole('dialog').getByRole('button', { name: 'Close', exact: true });
	await probe(close, 'processes-override'); await close.press('Enter'); await expect(page.getByRole('dialog')).toHaveCount(0);
	await go('/projects/atlas'); await page.getByRole('tab', { name: 'Bugs', exact: true }).click();
	await page.getByRole('button', { name: 'Report a bug', exact: true }).click();
	const smart = page.getByRole('dialog').getByRole('button', { name: 'Draft or improve Title', exact: true });
	await probe(smart, 'copilot'); await smart.press('Enter');
	await probe(page.getByTestId('copilot-bug-title').getByRole('button', { name: 'Draft', exact: true }), 'copilot-panel');
}
