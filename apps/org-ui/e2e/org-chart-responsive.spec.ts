import { expect, test, type Locator, type Page } from '@playwright/test';
import { visit } from './ready';
import { shot } from './shot';

async function openChart(page: Page, width: number, scale = 1, query = '') {
	await page.setViewportSize({ width, height: 640 });
	await visit(page, '/create');
	await page.getByLabel('Organization name').fill('Acme');
	await page.getByRole('button', { name: 'Create organization' }).click();
	await expect(page).toHaveURL(/\/agents$/);
	await visit(page, `/org-chart${query}`);
	await expect(page.getByRole('tablist', { name: 'Org chart views' })).toBeVisible();
	await page.addStyleTag({ content: `html { font-size: ${scale * 100}% !important; }` });
	await page.evaluate(() => document.fonts.ready.then(() => undefined));
}

async function expectControlsFit(page: Page, group: Locator) {
	const width = page.viewportSize()!.width;
	const scope = (await group.boundingBox())!;
	const boxes = await group.locator('button, input, select').evaluateAll((elements) => elements.map((element) => {
		const box = element.getBoundingClientRect(), range = document.createRange();
		range.selectNodeContents(element);
		return { label: element.textContent?.trim() ?? '', left: box.left, right: box.right,
			paintFits: [...range.getClientRects()].every((r) => r.left >= box.left - 1 && r.right <= box.right + 1 && r.top >= box.top - 1 && r.bottom <= box.bottom + 1) };
	}));
	expect(boxes.length).toBeGreaterThan(0);
	for (const box of boxes) {
		expect.soft(box.left, `${box.label} starts inside the viewport`).toBeGreaterThanOrEqual(0);
		expect.soft(box.right, `${box.label} ends inside the viewport`).toBeLessThanOrEqual(width);
		expect.soft(box.left, `${box.label} starts inside its group`).toBeGreaterThanOrEqual(scope.x - 1);
		expect.soft(box.right, `${box.label} ends inside its group`).toBeLessThanOrEqual(scope.x + scope.width + 1);
		expect.soft(box.paintFits, `${box.label} paints inside its control`).toBe(true);
	}
	expect(await page.evaluate(() => document.documentElement.scrollWidth <= document.documentElement.clientWidth), 'document stays inside its viewport').toBe(true);
	const main = page.getByRole('main');
	const frame = await main.evaluate((element) => ({
		width: element.clientWidth, scrollWidth: element.scrollWidth,
		overflow: [...element.querySelectorAll('*')].filter((child) => !child.closest('.overflow-x-auto') && child.getBoundingClientRect().right > innerWidth).map((child) => ({tag:child.tagName, text:child.textContent?.trim().slice(0,70), right:child.getBoundingClientRect().right, class:child.getAttribute('class')})).slice(-12)
	}));
	expect(frame.scrollWidth <= frame.width, `only the chart canvas may scroll horizontally: ${JSON.stringify(frame)}`).toBe(true);
}

for (const width of [320, 960, 1280]) {
	test(`BUG-2: every Teams action stays inside the viewport at ${width}px`, async ({ page }) => {
		await openChart(page, width);
		const panel = page.getByRole('tabpanel', { name: 'Teams', exact: true });
		const actions = panel.getByRole('button', { name: 'New team', exact: true }).locator('..');
		await expect(actions.getByRole('button')).toHaveText(['New team', 'Add person', 'Add agent', 'List', 'Actual size', 'Expand all', 'Collapse all']);
		await shot(page, `bug2-teams-toolbar-${width}`, { context: actions });
		await expectControlsFit(page, actions);
	});
}

for (const width of [320, 600, 639]) {
	test(`BUG-2: narrow Search members uses the available full content width at ${width}px`, async ({ page }) => {
		await openChart(page, width);
		const search = page.getByRole('searchbox', { name: 'Search members', exact: true });
		const content = page.getByRole('region', { name: 'Owner', exact: true }).locator('..');
		await shot(page, `bug2-search-${width}`, { context: search.locator('..') });
		const room = await content.evaluate((element) => {
			const box = element.getBoundingClientRect(), style = getComputedStyle(element);
			return { left: box.left + parseFloat(style.paddingLeft), right: box.right - parseFloat(style.paddingRight) };
		});
		const input = (await search.boundingBox())!;
		expect(input.x).toBeCloseTo(room.left, 0);
		expect(input.x + input.width).toBeCloseTo(room.right, 0);
	});
}

test('BUG-2: existing zoom, collapse, list and keyboard dialog actions remain functional', async ({ page }) => {
	await openChart(page, 1280);
	const chart = page.getByTestId('teams-chart');
	await expect(page.getByTestId('team-card')).toHaveCount(5);
	await page.getByRole('button', { name: 'Actual size', exact: true }).click();
	await expect(page.getByRole('button', { name: 'Fit to width', exact: true })).toBeVisible();
	const canvas = chart.locator('xpath=ancestor::div[contains(@class, "overflow-x-auto")][1]');
	expect(await canvas.evaluate((element) => element.scrollWidth > element.clientWidth)).toBe(true);
	await canvas.evaluate((element) => { element.scrollLeft = element.scrollWidth; });
	expect(await canvas.evaluate((element) => element.scrollLeft)).toBeGreaterThan(0);
	await page.getByRole('button', { name: 'Collapse all', exact: true }).click();
	await expect(page.getByTestId('team-card')).toHaveCount(3);
	await page.getByRole('button', { name: 'Expand all', exact: true }).click();
	await expect(page.getByTestId('team-card')).toHaveCount(5);
	await page.getByRole('button', { name: 'List', exact: true }).click();
	await expect(chart).toHaveCount(0);
	await page.getByRole('button', { name: 'Chart', exact: true }).click();
	await expect(chart).toBeVisible();
	const create = page.getByRole('button', { name: 'New team', exact: true });
	await create.press('Enter');
	await expect(page.getByRole('dialog')).toBeVisible();
	await page.keyboard.press('Escape');
	await expect(create).toBeFocused();
	await shot(page, 'bug2-toolbar-behavior-control');
});

for (const width of [320, 640, 720, 960, 1280]) {
	for (const scale of [1, 2]) {
		test(`BUG-2: all chart views fit at ${width}px and ${scale * 100}% text`, async ({ page }) => {
			await openChart(page, width, scale);
			const tabs = page.getByRole('tablist', { name: 'Org chart views' });
			const teams = page.getByRole('tabpanel', { name: 'Teams', exact: true });
			const actions = teams.getByRole('button', { name: 'New team', exact: true }).locator('..');
			await shot(page, `bug2-teams-${width}-${scale * 100}`, { context: actions });
			await expectControlsFit(page, actions);
			await expectControlsFit(page, tabs);
			const search = page.getByRole('searchbox', { name: 'Search members', exact: true });
			await expectControlsFit(page, search.locator('..'));
			await expectControlsFit(page, page.getByLabel('Office filter', { exact: true }).locator('..'));
			if (width < 640) {
				const room = await page.getByRole('region', { name: 'Owner', exact: true }).locator('..').evaluate((element) => {
					const b = element.getBoundingClientRect(), s = getComputedStyle(element);
					return { left: b.left + parseFloat(s.paddingLeft), right: b.right - parseFloat(s.paddingRight) };
				});
				const box = (await search.boundingBox())!;
				expect(box.x).toBeCloseTo(room.left, 0);
				expect(box.x + box.width).toBeCloseTo(room.right, 0);
			}
			await teams.getByRole('button', { name: 'List', exact: true }).click();
			await expect(page.getByTestId('teams-chart')).toHaveCount(0);
			await shot(page, `bug2-list-${width}-${scale * 100}`, { context: actions });
			await expectControlsFit(page, actions);
			await teams.getByRole('button', { name: 'Chart', exact: true }).click();
			await expect(page.getByTestId('teams-chart')).toBeVisible();
			await tabs.getByRole('tab', { name: 'People', exact: true }).click();
			const people = page.getByRole('tabpanel', { name: 'People', exact: true });
			const peopleControls = people.getByRole('button', { name: /^(Actual size|Fit to width)$/ }).locator('..');
			await shot(page, `bug2-people-${width}-${scale * 100}`, { context: peopleControls });
			await expectControlsFit(page, peopleControls);
			await expectControlsFit(page, people.getByRole('button', { name: 'Add person', exact: true }).locator('..'));
			await tabs.getByRole('tab', { name: 'Reporting lines', exact: true }).click();
			const reportingControls = page.getByRole('tabpanel', { name: 'Reporting lines', exact: true }).getByRole('button', { name: /^(Actual size|Fit to width)$/ }).locator('..');
			await shot(page, `bug2-reporting-${width}-${scale * 100}`, { context: reportingControls });
			await expectControlsFit(page, reportingControls);
		});
	}
}

for (const width of [320, 960]) {
	for (const scale of [1, 2]) {
		test(`BUG-2: wrapping keeps DOM and keyboard focus order at ${width}px and ${scale * 100}% text`, async ({ page, browserName }) => {
			await openChart(page, width, scale);
			const search = page.getByRole('searchbox', { name: 'Search members', exact: true });
			const panel = page.getByRole('tabpanel', { name: 'Teams', exact: true });
			const actions = panel.getByRole('button', { name: 'New team', exact: true }).locator('..');
			await shot(page, `bug2-order-${width}-${scale * 100}`, { context: actions });
			const positions = await actions.getByRole('button').evaluateAll((elements) => elements.map((element) => {
				const b = element.getBoundingClientRect(); return { x: b.x + b.width / 2, y: b.y + b.height / 2 };
			}));
			for (let index = 1; index < positions.length; index++) {
				const previous = positions[index - 1], current = positions[index];
				if (Math.abs(current.y - previous.y) < 1) expect(current.x).toBeGreaterThan(previous.x);
				else expect(current.y).toBeGreaterThan(previous.y);
			}
			const sequence = [search, page.getByRole('tab', { name: 'Teams', exact: true }), page.getByLabel('Office filter', { exact: true }), ...['New team', 'Add person', 'Add agent', 'List', 'Actual size', 'Expand all', 'Collapse all'].map((name) => actions.getByRole('button', { name, exact: true }))];
			// Start from a user-visible field; focus() alone does not wait for WebKit scrolling.
			await search.click();
			for (let index = 0; index < sequence.length; index++) {
				await expect(sequence[index]).toBeFocused();
				expect(await sequence[index].evaluate((element) => {
					const b = element.getBoundingClientRect(), hit = document.elementFromPoint(b.x + b.width / 2, b.y + b.height / 2);
					return b.top >= 0 && b.bottom <= innerHeight && !!hit && element.contains(hit);
				}), `focus step ${index} is visible and unobscured`).toBe(true);
				if (index === 3 || index === sequence.length - 1) await shot(page, `bug2-focus-${index}-${width}-${scale * 100}`);
				// Native macOS WebKit uses Option+Tab to include buttons in keyboard traversal.
				if (index + 1 < sequence.length) await page.keyboard.press(browserName === 'webkit' && process.platform === 'darwin' ? 'Alt+Tab' : 'Tab');
			}
			await expectControlsFit(page, actions);
		});
	}
}

test('BUG-2: the empty fixture keeps toolbar controls at 320px and 200% text', async ({ page }) => {
	await openChart(page, 320, 2, '?org=empty');
	await expect(page.getByText('No teams yet', { exact: true })).toBeVisible();
	await expect(page.getByTestId('teams-chart')).toHaveCount(0);
	const panel = page.getByRole('tabpanel', { name: 'Teams', exact: true });
	const actions = panel.getByRole('button', { name: 'New team', exact: true }).locator('..');
	await shot(page, 'bug2-empty-teams-320-200', { context: actions });
	await expectControlsFit(page, actions);
	await page.getByRole('tab', { name: 'People', exact: true }).click();
	await expect(page.getByText('No teams yet', { exact: true })).toBeVisible();
	const add = page.getByRole('tabpanel', { name: 'People', exact: true }).getByRole('button', { name: 'Add person', exact: true });
	await shot(page, 'bug2-empty-people-320-200', { context: add.locator('..') });
	await expectControlsFit(page, add.locator('..'));
});

test('BUG-2: a narrow no-match result clears with Escape and retains search focus', async ({ page }) => {
	await openChart(page, 320, 2);
	const search = page.getByRole('searchbox', { name: 'Search members', exact: true });
	await search.fill('zzzz');
	await expect(page.getByTestId('search-status')).toContainText('No one matches.');
	await shot(page, 'bug2-search-no-match-320-200', { context: search.locator('..').locator('..') });
	await expectControlsFit(page, search.locator('..').locator('..'));
	await search.press('Escape');
	await expect(search).toHaveValue('');
	await expect(search).toBeFocused();
	await expect(page.getByTestId('search-status')).toBeEmpty();
});

for (const region of ['sample note', 'unassigned-project row', 'empty call-to-action', 'TeamsList header'] as const) {
	test(`BUG-2: ${region} stays inside its parent at 320px and 200% text`, async ({ page }) => {
		await openChart(page, 320, 2, region === 'empty call-to-action' ? '?org=empty' : '');
		if (region === 'TeamsList header') await page.getByRole('button', { name: 'List', exact: true }).click();
		const targets = region === 'sample note' ? page.getByTestId('sample-data-note')
			: region === 'unassigned-project row' ? page.getByTestId('unassigned-projects').locator('li')
			: region === 'empty call-to-action' ? page.getByRole('button', { name: 'Create your first team', exact: true })
			: page.getByTestId('team-list-item').locator(':scope > p:first-child');
		await expect(targets.first()).toBeVisible();
		await shot(page, `bug2-region-${region.replaceAll(' ', '-')}-320-200`, { context: targets.first() });
		expect(await page.evaluate(() => document.documentElement.scrollWidth <= document.documentElement.clientWidth), `${region}: document stays inside its viewport`).toBe(true);
		const geometry = await targets.evaluateAll((elements) => elements.map((element) => {
			const parent = element.parentElement!, outer = parent.getBoundingClientRect(), style = getComputedStyle(parent);
			const left = outer.left + parseFloat(style.paddingLeft), right = outer.right - parseFloat(style.paddingRight);
			const range = document.createRange(); range.selectNodeContents(element);
			const rects = [element.getBoundingClientRect(), ...range.getClientRects()];
			return { left, right, rects: rects.map((r) => ({ left: r.left, right: r.right })) };
		}));
		for (const item of geometry) for (const rect of item.rects) {
			expect.soft(rect.left, `${region}: content starts inside available parent width`).toBeGreaterThanOrEqual(item.left - 1);
			expect.soft(rect.right, `${region}: content ends inside available parent width`).toBeLessThanOrEqual(item.right + 1);
		}
		expect(await page.getByRole('main').evaluate((element) => element.scrollWidth <= element.clientWidth), `${region}: main does not scroll horizontally`).toBe(true);
	});
}
