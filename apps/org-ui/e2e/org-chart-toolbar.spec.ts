import { expect, test, type Page } from '@playwright/test';
import { visit } from './ready';
import { shot } from './shot';
import { viewAction as chooseView } from './org-chart-menu';

async function open(page: Page, width = 1280, text = 1, query = '') {
	await page.setViewportSize({ width, height: 640 });
	await visit(page, '/create');
	await page.getByLabel('Organization name').fill('Acme');
	await page.getByRole('button', { name: 'Create organization' }).click();
	await expect(page).toHaveURL(/\/agents$/);
	await visit(page, '/org-chart' + query);
	await page.addStyleTag({ content: `html { font-size: ${text * 100}% !important; }` });
	await page.evaluate(() => document.fonts.ready.then(() => undefined));
}

async function closeBlockedMenu(page: Page) {
	const fit = page.getByRole('menuitem', { name: 'Fit to width', exact: true });
	await expect(page.getByRole('tooltip')).toBeVisible();
	await expect(fit).toBeFocused();
	await page.keyboard.press('Escape');
	await expect(page.getByRole('tooltip')).toHaveCount(0);
	await expect(fit).toBeFocused();
	await expect(page.getByRole('menu')).toBeVisible();
	await page.keyboard.press('Escape');
	await expect(page.getByRole('menu')).toHaveCount(0);
	await expect(page.getByRole('button', { name: 'View', exact: true })).toBeFocused();
}

test('OWNER-1 BUG-70 OC-2: owner is a normal avatar row without a crown or card', async ({ page }) => {
	await open(page);
	const owner = page.getByRole('region', { name: 'Owner', exact: true });
	await expect(owner).toContainText('Alex Kim');
	await expect(owner).toContainText('Owner');
	await expect(owner).toHaveCSS('background-color', 'rgba(0, 0, 0, 0)');
	await expect(owner.locator('svg')).toHaveCount(0);
	await expect(owner.locator('[aria-hidden="true"]')).toContainText('AK');
	await shot(page, 'oc1-normal-owner-row', { context: owner });
});

test('TEAM-1 TEAM-10 BUG-70 OC-4: View menu preserves list, fit and collapse commands', async ({ page }) => {
	await open(page);
	const panel = page.getByRole('tabpanel', { name: 'Teams', exact: true });
	await expect(panel.getByRole('button', { name: 'View', exact: true })).toBeVisible();
	await expect(panel.getByRole('button', { name: 'New team', exact: true })).toBeVisible();
	await expect(panel.getByRole('button', { name: 'Add person', exact: true })).toBeVisible();
	await expect(panel.getByRole('button', { name: 'Add agent', exact: true })).toBeVisible();
	await chooseView(page, 'Actual size');
	await expect(page.getByTestId('teams-chart')).toHaveCSS('transform', 'matrix(1, 0, 0, 1, 0, 0)');
	await chooseView(page, 'Collapse all');
	await expect(page.getByTestId('team-card')).toHaveCount(3);
	await chooseView(page, 'Expand all');
	await expect(page.getByTestId('team-card')).toHaveCount(5);
	await chooseView(page, 'List');
	await expect(page.getByTestId('team-list-item')).toHaveCount(5);
	await expect(page.getByTestId('teams-chart')).toHaveCount(0);
	await chooseView(page, 'Chart');
	await expect(page.getByTestId('teams-chart')).toBeVisible();
	await chooseView(page, 'Fit to width');
	await shot(page, 'oc1-view-menu-actions');
});

test('TEAM-14 BUG-70 OC-5: unassigned project copy is a quiet Callout', async ({ page }) => {
	await open(page);
	const note = page.getByTestId('unassigned-note');
	await expect(note).toHaveText('1 project has no team yet.');
	await expect(note).not.toHaveAttribute('role');
	await expect(note).toHaveAttribute('data-slot', 'alert');
	await expect(note).toHaveCSS('border-top-width', '1px');
	await shot(page, 'oc1-quiet-unassigned-note', { context: note });
});

for (const width of [320, 639, 640]) {
	test(`TEAM-10 BUG-70 OC-9: initial view follows the 640px boundary at ${width}px`, async ({ page }) => {
		await open(page, width);
		await expect(page.getByTestId('team-list-item')).toHaveCount(width < 640 ? 5 : 0);
		await expect(page.getByTestId('teams-chart')).toHaveCount(width < 640 ? 0 : 1);
		await shot(page, `oc1-initial-${width}`);
	});
}

test('TEAM-1 BUG-70 BUG-90: at 320x640 the initial chart content starts in the first viewport', async ({ page }) => {
	await open(page, 320);
	const first = page.locator('[data-testid=team-list-item], [data-testid=team-card]').first();
	await expect(first).toBeVisible();
	expect((await first.boundingBox())!.y).toBeLessThan(640);
	await shot(page, 'oc1-phone-first-viewport');
});

for (const text of [1, 2]) {
	test(`PEOPLE-1 BUG-70 BUG-90: People Fit keeps name and role lines readable at 320px and ${text * 100}% text`, async ({ page }) => {
		await open(page, 320, text);
		await page.getByRole('tab', { name: 'People', exact: true }).click();
		const chart = page.getByTestId('people-chart');
		expect(await chart.locator('h3, p, li > span.truncate').evaluateAll((elements) => elements.length > 0 && elements.every((element) => element.hasAttribute('data-fit-text')))).toBe(true);
		await expect.poll(() => chart.evaluate((element) => {
			const scale = new DOMMatrix(getComputedStyle(element).transform).a;
			return Math.min(...[...element.querySelectorAll('[data-fit-text]')].map((e) => parseFloat(getComputedStyle(e).fontSize) * scale));
		})).toBeGreaterThanOrEqual(12);
		// Wait for the observer's measured Fit result, rather than accepting the unmeasured scale=1.
		await expect.poll(() => chart.evaluate((e) => new DOMMatrix(getComputedStyle(e).transform).a)).toBe(1 / text);
		const frame = page.getByRole('region', { name: 'People chart', exact: true });
		await expect(frame).toHaveAttribute('tabindex', '0');
		await expect(frame).toHaveCSS('overflow-x', 'auto');
		expect(await frame.evaluate((e) => e.scrollWidth > e.clientWidth)).toBe(true);
		await frame.focus();
		await expect(frame).toBeFocused();
		await frame.press('ArrowRight');
		await expect.poll(() => frame.evaluate((e) => e.scrollLeft)).toBeGreaterThan(0);
		await frame.press('ArrowLeft');
		await expect.poll(() => frame.evaluate((e) => e.scrollLeft)).toBe(0);
		await shot(page, `oc1-people-readable-fit-${text}`, { context: frame });
	});
}


test('TEAM-1 BUG-70 OC-9: a chart frame is named, focusable and visibly outlined', async ({ page }) => {
	await open(page);
	const frame = page.getByRole('region', { name: 'Teams chart', exact: true });
	await expect(frame).toHaveAttribute('tabindex', '0');
	await expect(frame).toHaveCSS('overflow-x', 'auto');
	await frame.focus();
	await expect(frame).toHaveCSS('outline-width', '2px');
	await shot(page, 'oc1-focusable-chart-frame', { context: frame });
});

test('TEAM-1 PEOPLE-1 REPORT-2 BUG-70 OC-10: chart metadata uses gray-400 rather than gray-500', async ({ page }) => {
	await open(page);
	for (const view of ['Teams', 'People', 'Reporting lines']) {
		await page.getByRole('tab', { name: view, exact: true }).click();
		const canvas = page.getByTestId(view === 'Teams' ? 'teams-chart' : view === 'People' ? 'people-chart' : 'reporting-chart');
		await expect(canvas.locator('.text-gray-500')).toHaveCount(0);
		await shot(page, 'oc1-meta-' + view.replaceAll(' ', '-').toLowerCase());
	}
});

test('PEOPLE-1 BUG-70 BUG-90: team lead is in the member accessible name and the badge is 12px at Actual size', async ({ page }) => {
	await open(page, 2560);
	await page.getByRole('tab', { name: 'People', exact: true }).click();
	const card = page.getByTestId('people-card').filter({ hasText: 'Hana Weiss' });
	await expect(card.getByRole('button', { name: 'Hana Weiss, person, team lead', exact: true })).toBeVisible();
	await expect(card.getByRole('button', { name: 'Leo Brandt, person', exact: true })).toBeVisible();
	await expect(card.getByRole('button', { name: /Leo Brandt.*team lead/ })).toHaveCount(0);
	await chooseView(page, 'Actual size');
	await expect(card.getByText('lead', { exact: true })).toHaveCSS('font-size', '12px');
	await expect(card.getByText('lead', { exact: true })).toHaveAttribute('data-fit-text', '');
	await page.getByRole('tab', { name: 'Teams', exact: true }).click();
	await expect(page.getByTestId('team-card').filter({ hasText: 'Hana Weiss' }).getByRole('button', { name: 'Hana Weiss, person', exact: true })).toBeVisible();
	await shot(page, 'oc1-lead-name-and-unmodified-other-views');
});

test('TEAM-10 TEAM-11 BUG-70 OC-9: responsive defaults change at 640 but explicit choices survive resizes and tabs', async ({ page }) => {
	await open(page, 639);
	await expect(page.getByTestId('team-list-item')).toHaveCount(5);
	await page.setViewportSize({ width: 640, height: 640 });
	await expect(page.getByTestId('teams-chart')).toBeVisible();
	await page.setViewportSize({ width: 639, height: 640 });
	await expect(page.getByTestId('teams-chart')).toHaveCount(0);
	await chooseView(page, 'Chart');
	await page.setViewportSize({ width: 640, height: 640 });
	await page.setViewportSize({ width: 320, height: 640 });
	await expect(page.getByTestId('teams-chart')).toBeVisible();
	await page.getByRole('tab', { name: 'People', exact: true }).click();
	await page.getByRole('tab', { name: 'Teams', exact: true }).click();
	await expect(page.getByTestId('teams-chart')).toBeVisible();
	await chooseView(page, 'List');
	await page.setViewportSize({ width: 1280, height: 640 });
	await page.getByRole('button', { name: 'View', exact: true }).click();
	await expect(page.getByRole('menuitem')).toHaveText(['Chart']);
	await page.keyboard.press('Escape');
	await expect(page.getByTestId('team-list-item')).toHaveCount(5);
	await visit(page, '/agents');
	await visit(page, '/org-chart');
	await expect(page.getByTestId('teams-chart')).toBeVisible();
	await shot(page, 'oc1-explicit-view-lifetime');
});

test('TEAM-1 TEAM-11 PEOPLE-1 REPORT-1 BUG-70 OC-4: View keyboard navigation, focus return and independent collapse state', async ({ page }) => {
	await open(page);
	const trigger = page.getByRole('button', { name: 'View', exact: true });
	await trigger.focus();
	await trigger.press('Enter');
	await expect(page.getByRole('menuitem').first()).toBeFocused();
	await page.keyboard.press('ArrowDown');
	await expect(page.getByRole('menuitem', { name: 'Actual size', exact: true })).toBeFocused();
	await page.keyboard.press('Escape');
	await expect(trigger).toBeFocused();
	await trigger.press('Enter');
	await expect(page.getByRole('menuitem').first()).toBeFocused();
	await page.keyboard.press('End');
	await expect(page.getByRole('menuitem', { name: 'Collapse all', exact: true })).toBeFocused();
	await shot(page, 'oc1-keyboard-view-menu');
	await page.keyboard.press('Enter');
	await expect(page.getByTestId('team-card')).toHaveCount(3);
	await expect(trigger).toBeFocused();
	await page.getByRole('tab', { name: 'People', exact: true }).click();
	await expect(page.getByTestId('people-card')).toHaveCount(3);
	await chooseView(page, 'Expand all');
	await expect(page.getByTestId('people-card')).toHaveCount(5);
	await page.getByRole('tab', { name: 'Reporting lines', exact: true }).click();
	await expect(page.getByTestId('report-node')).toHaveCount(12);
	await chooseView(page, 'Collapse all');
	await expect(page.getByTestId('report-node')).toHaveCount(3);
	await page.getByRole('tab', { name: 'Teams', exact: true }).click();
	await expect(page.getByTestId('team-card')).toHaveCount(5);
	await page.getByRole('tab', { name: 'Reporting lines', exact: true }).click();
	await expect(page.getByTestId('report-node')).toHaveCount(3);
	await chooseView(page, 'Expand all');
	await expect(page.getByTestId('report-node')).toHaveCount(12);
	await chooseView(page, 'Actual size');
	await expect(page.getByTestId('reporting-chart')).toHaveCSS('transform', 'matrix(1, 0, 0, 1, 0, 0)');
	await page.getByRole('tab', { name: 'People', exact: true }).click();
	await trigger.click();
	await expect(page.getByRole('menuitem')).toHaveText(['Fit to width', 'Expand all', 'Collapse all']);
	await page.keyboard.press('Escape');
	await expect(trigger).toBeFocused();
});

for (const width of [320, 390, 640, 960, 1280]) for (const text of [1, 2]) {
	test(`TEAM-1 TEAM-10 TEAM-14 BUG-70 OC-4 OC-9: all View menus fit at ${width}px and ${text * 100}% text`, async ({ page }) => {
		await open(page, width, text);
		for (const view of ['Teams', 'People', 'Reporting lines']) {
			await page.getByRole('tab', { name: view, exact: true }).click();
			const trigger = page.getByRole('button', { name: 'View', exact: true });
			await trigger.click();
			const menu = page.getByRole('menu');
			await expect(menu).toBeVisible();
			await expect(menu.getByRole('menuitem')).toHaveText(view === 'Teams' ? (width < 640 ? ['Chart'] : ['List', 'Actual size', 'Expand all', 'Collapse all']) : [view === 'People' && text === 1 ? 'Fit to width' : 'Actual size', 'Expand all', 'Collapse all']);
			const box = (await menu.boundingBox())!;
			expect(box.x).toBeGreaterThanOrEqual(-1);
			expect(box.x + box.width).toBeLessThanOrEqual(width + 1);
			expect(await menu.getByRole('menuitem').evaluateAll((items) => items.every((item) => {
				const box = item.getBoundingClientRect(), text = document.createRange();
				text.selectNodeContents(item);
				return [...text.getClientRects()].every((r) => r.left >= box.left - 1 && r.right <= box.right + 1);
			}))).toBe(true);
			await shot(page, `oc1-menu-${view.replaceAll(' ', '-')}-${width}-${text}`, { context: menu });
			await page.keyboard.press('Escape');
			await expect(trigger).toBeFocused();
			expect(await page.getByRole('main').evaluate((e) => e.scrollWidth <= e.clientWidth)).toBe(true);
			expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
		}
	});
}

test('PEOPLE-1 BUG-70 BUG-90: People Fit caps at actual size in a wide frame', async ({ page }) => {
	await open(page, 2560);
	await page.getByRole('tab', { name: 'People', exact: true }).click();
	const canvas = page.getByTestId('people-chart');
	await expect.poll(() => canvas.evaluate((e) => new DOMMatrix(getComputedStyle(e).transform).a)).toBe(1);
	await chooseView(page, 'Actual size');
	await expect(canvas).toHaveCSS('transform', 'matrix(1, 0, 0, 1, 0, 0)');
	await chooseView(page, 'Fit to width');
	await expect(canvas).toHaveCSS('transform', 'matrix(1, 0, 0, 1, 0, 0)');
	await shot(page, 'oc1-fit-wide-actual-cap');
});

test('TEAM-1 TEAM-5 PEOPLE-1 BUG-70 OC-4 OC-5: empty mobile view retains creation actions and hides unassigned context', async ({ page }) => {
	await open(page, 320, 2, '?org=empty');
	await expect(page.getByText('No teams yet', { exact: true })).toBeVisible();
	await expect(page.getByTestId('unassigned-note')).toHaveCount(0);
	const view = page.getByRole('button', { name: 'View', exact: true });
	await view.click();
	await expect(page.getByRole('menuitem')).toHaveText(['Chart']);
	await page.keyboard.press('Escape');
	await expect(view).toBeFocused();
	const create = page.getByRole('button', { name: 'Create your first team', exact: true });
	await create.press('Enter');
	await expect(page.getByRole('dialog')).toBeVisible();
	await page.keyboard.press('Escape');
	await expect(create).toBeFocused();
	await page.getByRole('tab', { name: 'People', exact: true }).click();
	await expect(page.getByRole('button', { name: 'View', exact: true })).toHaveCount(0);
	await page.getByRole('button', { name: 'Add person', exact: true }).press('Enter');
	await expect(page.getByRole('dialog')).toBeVisible();
	await page.keyboard.press('Escape');
	await expect(page.getByRole('tabpanel', { name: 'People', exact: true }).getByRole('button', { name: 'Add person', exact: true })).toBeFocused();
	await shot(page, 'oc1-empty-people-320-200');
});


for (const scenario of [
	{ width: 320, text: 1, unavailable: true },
	{ width: 320, text: 2, unavailable: false },
	{ width: 2560, text: 1, unavailable: false }
]) {
	test(`PEOPLE-1 BUG-70 BUG-90: Fit availability at ${scenario.width}px and ${scenario.text * 100}% text`, async ({ page }) => {
		await open(page, scenario.width, scenario.text);
		await page.getByRole('tab', { name: 'People', exact: true }).click();
		const canvas = page.getByTestId('people-chart');
		await expect.poll(() => canvas.evaluate((e) => new DOMMatrix(getComputedStyle(e).transform).a)).toBe(1 / scenario.text);
		const trigger = page.getByRole('button', { name: 'View', exact: true });
		if (!scenario.unavailable) await chooseView(page, 'Actual size');
		await trigger.press('Enter');
		const fit = page.getByRole('menuitem', { name: 'Fit to width', exact: true });
		await expect(fit).toBeFocused();
		if (scenario.unavailable) {
			await expect(fit).toHaveAttribute('aria-disabled', 'true');
			await expect(fit).toHaveAccessibleDescription('The chart is already at its smallest readable size. Scroll to see the rest.');
			await expect(page.getByRole('tooltip')).toHaveText('The chart is already at its smallest readable size. Scroll to see the rest.');
			await shot(page, 'oc1-fit-unavailable-tooltip', { context: page.getByRole('menu') });
			await fit.press('Enter');
			await expect(page.getByRole('menu')).toBeVisible();
			await expect(fit).toBeFocused();
			await expect(canvas).toHaveCSS('transform', 'matrix(1, 0, 0, 1, 0, 0)');
			await page.keyboard.press('ArrowDown');
			await expect(page.getByRole('menuitem', { name: 'Expand all', exact: true })).toBeFocused();
			await page.keyboard.press('Escape');
		} else {
			await expect(fit).not.toHaveAttribute('aria-disabled', 'true');
			await fit.press('Enter');
			await expect(page.getByRole('menu')).toHaveCount(0);
			await expect.poll(() => canvas.evaluate((e) => new DOMMatrix(getComputedStyle(e).transform).a)).toBe(1 / scenario.text);
		}
		await expect(trigger).toBeFocused();
		if (scenario.unavailable) {
			await page.setViewportSize({ width: 2560, height: 640 });
			await trigger.click();
			await expect(page.getByRole('menuitem', { name: 'Actual size', exact: true })).toBeVisible();
			await page.keyboard.press('Escape');
		}
	});
}

test('PEOPLE-1 BUG-70 BUG-90: Fit availability tracks viewport and text while preserving the last explicit fit choice', async ({ page }) => {
	await open(page, 320);
	await page.getByRole('tab', { name: 'People', exact: true }).click();
	const canvas = page.getByTestId('people-chart');
	const trigger = page.getByRole('button', { name: 'View', exact: true });
	await trigger.press('Enter');
	await expect(page.getByRole('menuitem', { name: 'Fit to width', exact: true })).toHaveAttribute('aria-disabled', 'true');
	await closeBlockedMenu(page);
	await page.setViewportSize({ width: 2560, height: 640 });
	await trigger.click();
	await expect(page.getByRole('menuitem', { name: 'Actual size', exact: true })).not.toHaveAttribute('aria-disabled', 'true');
	await page.getByRole('menuitem', { name: 'Actual size', exact: true }).click();
	await page.setViewportSize({ width: 320, height: 640 });
	await trigger.press('Enter');
	const blocked = page.getByRole('menuitem', { name: 'Fit to width', exact: true });
	await expect(blocked).toHaveAttribute('aria-disabled', 'true');
	await expect(blocked).toBeFocused();
	// The observer can enable the tooltip after the item received focus. Re-enter it now blocked.
	await blocked.press('ArrowDown');
	await expect(page.getByRole('menuitem', { name: 'Expand all', exact: true })).toBeFocused();
	await page.keyboard.press('ArrowUp');
	await expect(blocked).toBeFocused();
	await closeBlockedMenu(page);
	await page.addStyleTag({ content: 'html { font-size: 200% !important; }' });
	await trigger.press('Enter');
	const fit = page.getByRole('menuitem', { name: 'Fit to width', exact: true });
	await expect(fit).not.toHaveAttribute('aria-disabled', 'true');
	await expect(canvas).toHaveCSS('transform', 'matrix(1, 0, 0, 1, 0, 0)');
	await fit.press('Enter');
	await expect.poll(() => canvas.evaluate((e) => new DOMMatrix(getComputedStyle(e).transform).a)).toBe(.5);
	await page.addStyleTag({ content: 'html { font-size: 100% !important; }' });
	await expect(canvas).toHaveCSS('transform', 'matrix(1, 0, 0, 1, 0, 0)');
	await page.setViewportSize({ width: 2560, height: 640 });
	await trigger.click();
	await expect(page.getByRole('menuitem', { name: 'Actual size', exact: true })).toBeVisible();
	await page.keyboard.press('Escape');
	await shot(page, 'oc1-fit-choice-after-availability-changes');
});

test('PEOPLE-1 BUG-70 BUG-90: a chart exactly as wide as its frame keeps Fit available', async ({ page }) => {
	await open(page, 2560);
	await page.getByRole('tab', { name: 'People', exact: true }).click();
	const frame = page.getByRole('region', { name: 'People chart', exact: true });
	const canvas = page.getByTestId('people-chart');
	await expect(canvas).toBeVisible();
	const excess = await frame.evaluate((e) => (e.firstElementChild as HTMLElement).clientWidth - e.querySelector('[data-testid=people-chart]')!.getBoundingClientRect().width);
	await page.setViewportSize({ width: Math.round(2560 - excess), height: 640 });
	await expect.poll(() => frame.evaluate((e) => Math.abs((e.firstElementChild as HTMLElement).clientWidth - e.querySelector('[data-testid=people-chart]')!.getBoundingClientRect().width))).toBeLessThan(.01);
	await page.getByRole('button', { name: 'View', exact: true }).click();
	await expect(page.getByRole('menuitem', { name: 'Actual size', exact: true })).not.toHaveAttribute('aria-disabled', 'true');
	await page.keyboard.press('Escape');
	await shot(page, 'oc1-fit-exact-width-boundary');
});


test('PEOPLE-1 BUG-70 BUG-90: clicking outside dismisses both the Fit explanation and View menu', async ({ page }) => {
	await open(page, 320);
	await page.getByRole('tab', { name: 'People', exact: true }).click();
	const trigger = page.getByRole('button', { name: 'View', exact: true });
	await trigger.press('Enter');
	await expect(page.getByRole('menuitem', { name: 'Fit to width', exact: true })).toBeFocused();
	await expect(page.getByRole('tooltip')).toBeVisible();
	await shot(page, 'oc1-fit-before-outside-dismissal');
	const heading = await page.getByRole('heading', { name: 'Org chart', exact: true }).boundingBox();
	expect(heading).not.toBeNull();
	await page.mouse.click(heading!.x + heading!.width / 2, heading!.y + heading!.height / 2);
	await expect(page.getByRole('menu')).toHaveCount(0);
	await expect(page.getByRole('tooltip')).toHaveCount(0);
	await shot(page, 'oc1-fit-after-outside-dismissal');
});


for (const text of [1, 2]) {
	test(`TEAM-2 PEOPLE-1 BUG-70 BUG-90: secondary-team badges stay readable at ${text * 100}% text`, async ({ page }) => {
		await open(page, 320, text);
		const response = await page.request.get('/e2e/fakes/org-api.ts');
		expect(response.ok()).toBe(true);
		const module = await response.text();
		await page.route('**/e2e/fakes/org-api.ts*', (route) => route.fulfill({ contentType: 'text/javascript', body: module + `
			const oc1View = orgApi.view.bind(orgApi);
			orgApi.view = async (...args) => {
				const view = await oc1View(...args);
				return { ...view, snapshot: { ...view.snapshot, memberships: [...view.snapshot.memberships, { id: 'oc1-leo-web', team_id: 'web', member_id: 'leo', version: 1 }] } };
			};` }));
		await visit(page, '/org-chart');
		await page.addStyleTag({ content: `html { font-size: ${text * 100}% !important; }` });
		await page.getByRole('tab', { name: 'People', exact: true }).click();
		const people = page.getByTestId('people-card').filter({ has: page.getByRole('heading', { name: 'People', exact: true, level: 3 }) });
		await expect(people.getByRole('button', { name: 'Leo Brandt, person, also in Web', exact: true })).toBeVisible();
		const badge = people.getByText('+1 team', { exact: true });
		await expect(badge).toHaveCSS('font-size', `${12 * text}px`);
		await expect(badge).toHaveAttribute('data-fit-text', '');
		await expect.poll(() => page.getByTestId('people-chart').evaluate((e) => new DOMMatrix(getComputedStyle(e).transform).a)).toBe(1 / text);
		expect(await badge.evaluate((e) => parseFloat(getComputedStyle(e).fontSize) * new DOMMatrix(getComputedStyle(e.closest('[data-testid=people-chart]')!).transform).a)).toBeGreaterThanOrEqual(12);
		await shot(page, `oc1-secondary-team-People-${text}`, { context: people });
		// Capture the same shared tile at Actual size with enough room to review its text.
		await page.setViewportSize({ width: 1280, height: 900 });
		await page.getByRole('tab', { name: 'Teams', exact: true }).click();
		await expect(page.getByTestId('teams-chart')).toBeVisible();
		await chooseView(page, 'Actual size');
		const team = page.getByTestId('team-card').filter({ has: page.getByRole('heading', { name: 'People', exact: true, level: 3 }) });
		const leo = team.getByRole('button', { name: 'Leo Brandt, person, also in Web', exact: true });
		await expect(leo).toBeVisible();
		await expect(leo.locator('..').getByText('+1 team', { exact: true })).toHaveCSS('font-size', `${12 * text}px`);
		await shot(page, `oc1-secondary-team-Teams-${text}`, { context: team });
		await page.getByRole('tab', { name: 'Reporting lines', exact: true }).click();
		const report = page.locator('[data-testid=report-node][data-member=leo]');
		await expect(report.getByRole('button', { name: 'Leo Brandt, person', exact: true })).toBeVisible();
		await report.scrollIntoViewIfNeeded();
		await shot(page, `oc1-secondary-team-Reporting-${text}`, { context: report });
	});
}


test('PEOPLE-1 BUG-70 BUG-90: a live wide-to-narrow Fit change keeps the same focused menu item', async ({ page }) => {
	await open(page, 2560);
	await page.getByRole('tab', { name: 'People', exact: true }).click();
	const trigger = page.getByRole('button', { name: 'View', exact: true });
	await trigger.press('Enter');
	const actual = page.getByRole('menuitem', { name: 'Actual size', exact: true });
	await expect(actual).toBeFocused();
	const id = await actual.getAttribute('id');
	await page.setViewportSize({ width: 320, height: 640 });
	const fit = page.getByRole('menuitem', { name: 'Fit to width', exact: true });
	await expect(fit).toHaveAttribute('aria-disabled', 'true');
	await expect(fit).toHaveAttribute('id', id!);
	await expect(fit).toBeFocused();
	await expect(fit).toHaveAccessibleDescription('The chart is already at its smallest readable size. Scroll to see the rest.');
	await fit.press('ArrowDown');
	await expect(page.getByRole('menuitem', { name: 'Expand all', exact: true })).toBeFocused();
	await page.keyboard.press('ArrowUp');
	await expect(fit).toBeFocused();
	await expect(page.getByRole('tooltip')).toBeVisible();
	await shot(page, 'oc1-live-fit-change-focus');
	await closeBlockedMenu(page);
});
