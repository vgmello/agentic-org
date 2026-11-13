import type { OrgSnapshot } from '../src/lib/proto';
import { viewAction, expectViewAction } from './org-chart-menu';
import { expect, test, type Locator, type Page } from '@playwright/test';
import { visit } from './ready';
import { shot } from './shot';

// Firefox (Playwright 1.65, build 1572) segfaults in libxul when a getByRole('tabpanel', { name }) locator, named by
// aria-labelledby, is measured (shot's boundingBox) and then evaluateAll'd (BUG-191), so these panels are found by id.
const tabPanel = (page: Page, id: 'teams' | 'people' | 'reporting') => page.locator(`#panel-${id}[role=tabpanel]`);

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
	// These legacy chart regressions choose Chart explicitly; OC-9 tests the mobile default separately.
	if (width < 640) await viewAction(page, 'Chart');
}

async function expectControlsFit(page: Page, group: Locator) {
	const width = page.viewportSize()!.width;
	// One in-page read of the group and its controls: Firefox segfaults on boundingBox() followed by evaluateAll() under
	// a role locator with a name (BUG-191).
	const { scope, boxes } = await group.evaluate((root) => {
		const g = root.getBoundingClientRect();
		return { scope: { x: g.x, width: g.width }, boxes: [...root.querySelectorAll('button, input, select')].map((element) => {
			const box = element.getBoundingClientRect(), range = document.createRange();
			range.selectNodeContents(element);
			return { label: element.textContent?.trim() ?? '', left: box.left, right: box.right,
				paintFits: [...range.getClientRects()].every((r) => r.left >= box.left - 1 && r.right <= box.right + 1 && r.top >= box.top - 1 && r.bottom <= box.bottom + 1) };
		}) };
	});
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
	test(`BUG-2: every Teams action stays inside the viewport at ${width}px`, { tag: '@webkit' }, async ({ page }) => {
		await openChart(page, width);
		const panel = tabPanel(page, 'teams');
		const actions = panel.getByRole('button', { name: 'New team', exact: true }).locator('..');
		await expect(actions.getByRole('button')).toHaveText(['New team', 'Add person', 'Add agent', 'View']);
		await shot(page, `bug2-teams-toolbar-${width}`, { context: actions });
		await expectControlsFit(page, actions);
	});
}

for (const width of [320, 600, 639]) {
	test(`BUG-2: narrow Search members uses the available full content width at ${width}px`, { tag: '@webkit' }, async ({ page }) => {
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

test('BUG-2: existing zoom, collapse, list and keyboard dialog actions remain functional', { tag: '@webkit' }, async ({ page }) => {
	await openChart(page, 1280);
	const chart = page.getByTestId('teams-chart');
	await expect(page.getByTestId('team-card')).toHaveCount(5);
	await viewAction(page, 'Actual size');
	await expectViewAction(page, 'Fit to width');
	const canvas = chart.locator('xpath=ancestor::div[contains(@class, "overflow-x-auto")][1]');
	expect(await canvas.evaluate((element) => element.scrollWidth > element.clientWidth)).toBe(true);
	await canvas.evaluate((element) => { element.scrollLeft = element.scrollWidth; });
	expect(await canvas.evaluate((element) => element.scrollLeft)).toBeGreaterThan(0);
	await viewAction(page, 'Collapse all');
	await expect(page.getByTestId('team-card')).toHaveCount(3);
	await viewAction(page, 'Expand all');
	await expect(page.getByTestId('team-card')).toHaveCount(5);
	await viewAction(page, 'List');
	await expect(chart).toHaveCount(0);
	await viewAction(page, 'Chart');
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
		test(`BUG-2: all chart views fit at ${width}px and ${scale * 100}% text`, { tag: '@webkit' }, async ({ page }) => {
			await openChart(page, width, scale);
			const tabs = page.getByRole('tablist', { name: 'Org chart views' });
			const teams = tabPanel(page, 'teams');
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
			await viewAction(page, 'List');
			await expect(page.getByTestId('teams-chart')).toHaveCount(0);
			await shot(page, `bug2-list-${width}-${scale * 100}`, { context: actions });
			await expectControlsFit(page, actions);
			await viewAction(page, 'Chart');
			await expect(page.getByTestId('teams-chart')).toBeVisible();
			await tabs.getByRole('tab', { name: 'People', exact: true }).click();
			const people = tabPanel(page, 'people');
			const peopleControls = people.getByRole('button', { name: 'View', exact: true }).locator('..');
			await shot(page, `bug2-people-${width}-${scale * 100}`, { context: peopleControls });
			await expectControlsFit(page, peopleControls);
			await expectControlsFit(page, people.getByRole('button', { name: 'Add person', exact: true }).locator('..'));
			await tabs.getByRole('tab', { name: 'Reporting lines', exact: true }).click();
			const reportingControls = tabPanel(page, 'reporting').getByRole('button', { name: 'View', exact: true }).locator('..');
			await shot(page, `bug2-reporting-${width}-${scale * 100}`, { context: reportingControls });
			await expectControlsFit(page, reportingControls);
		});
	}
}

for (const width of [320, 960]) {
	for (const scale of [1, 2]) {
		test(`BUG-2: wrapping keeps DOM and keyboard focus order at ${width}px and ${scale * 100}% text`, { tag: '@webkit' }, async ({ page, browserName }) => {
			await openChart(page, width, scale);
			const search = page.getByRole('searchbox', { name: 'Search members', exact: true });
			const panel = tabPanel(page, 'teams');
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
			const sequence = [search, page.getByRole('tab', { name: 'Teams', exact: true }), page.getByLabel('Office filter', { exact: true }), ...['New team', 'Add person', 'Add agent', 'View'].map((name) => actions.getByRole('button', { name, exact: true }))];
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

test('BUG-2: the empty fixture keeps toolbar controls at 320px and 200% text', { tag: '@webkit' }, async ({ page }) => {
	await openChart(page, 320, 2, '?org=empty');
	await expect(page.getByText('No teams yet', { exact: true })).toBeVisible();
	await expect(page.getByTestId('teams-chart')).toHaveCount(0);
	const panel = tabPanel(page, 'teams');
	const actions = panel.getByRole('button', { name: 'New team', exact: true }).locator('..');
	await shot(page, 'bug2-empty-teams-320-200', { context: actions });
	await expectControlsFit(page, actions);
	await page.getByRole('tab', { name: 'People', exact: true }).click();
	await expect(page.getByText('No teams yet', { exact: true })).toBeVisible();
	const add = tabPanel(page, 'people').getByRole('button', { name: 'Add person', exact: true });
	await shot(page, 'bug2-empty-people-320-200', { context: add.locator('..') });
	await expectControlsFit(page, add.locator('..'));
});

test('BUG-2: a narrow no-match result clears with Escape and retains search focus', { tag: '@webkit' }, async ({ page }) => {
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
	test(`BUG-2: ${region} stays inside its parent at 320px and 200% text`, { tag: '@webkit' }, async ({ page }) => {
		await openChart(page, 320, 2, region === 'empty call-to-action' ? '?org=empty' : '');
		if (region === 'TeamsList header') await viewAction(page, 'List');
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


for (const view of [
	{ path: '/agents?q=Noor', current: 'Agents' },
	{ path: '/org-chart?member=noor', current: 'Org chart' }
]) {
	test(`AGENT-3 TEAM-11 BUG-70 S2: ${view.current} has a separate plain-link Agents views navigation`, { tag: '@webkit' }, async ({ page }) => {
		await openChart(page, 1280);
		await visit(page, view.path);
		const nav = page.getByRole('navigation', { name: 'Agents views', exact: true });
		await expect(nav).toBeVisible();
		await expect(nav.getByRole('link')).toHaveText(['Agents', 'Org chart']);
		await expect(nav.getByRole('link', { name: 'Agents', exact: true })).toHaveAttribute('href', '/agents');
		await expect(nav.getByRole('link', { name: 'Org chart', exact: true })).toHaveAttribute('href', '/org-chart');
		await expect(nav.locator('[aria-current="page"]')).toHaveText(view.current);
		await expect(nav.locator('[aria-current]')).toHaveCount(1);
		await expect(nav.getByRole('tablist')).toHaveCount(0);
		await expect(nav.getByRole('tab')).toHaveCount(0);
		await expect(nav).not.toHaveAttribute('role', 'tablist');
		await shot(page, 'agents-views-' + view.current.replace(' ', '-').toLowerCase(), { context: nav });
	});
}


test('AGENT-3 TEAM-11 BUG-70 S2: keyboard links change routes, clear query state and preserve browser history', { tag: '@webkit' }, async ({ page, browserName }) => {
	await openChart(page, 1280);
	await visit(page, '/agents?q=Noor#saved');
	let nav = page.getByRole('navigation', { name: 'Agents views', exact: true });
	await nav.getByRole('link', { name: 'Org chart', exact: true }).click();
	await expect(page).toHaveURL(/\/org-chart$/);
	await page.goBack();
	await expect(page).toHaveURL(/\/agents\?q=Noor#saved$/);
	await expect(nav.locator('[aria-current="page"]')).toHaveText('Agents');
	await page.goForward();
	await expect(page).toHaveURL(/\/org-chart$/);
	await page.reload();
	await expect(nav.locator('[aria-current="page"]')).toHaveText('Org chart');
	await visit(page, '/org-chart?member=noor&q=discard#saved');
	await expect(page.getByTestId('selected-member')).toContainText('Noor Haddad');
	await nav.getByRole('link', { name: 'Agents', exact: true }).click();
	await expect(page).toHaveURL(/\/agents$/);
	const first = nav.getByRole('link', { name: 'Agents', exact: true });
	await first.focus();
	await expect(first).toBeFocused();
	await page.keyboard.press(browserName === 'webkit' && process.platform === 'darwin' ? 'Alt+Tab' : 'Tab');
	const next = nav.getByRole('link', { name: 'Org chart', exact: true });
	await expect(next).toBeFocused();
	await expect(next).toHaveCSS('outline-width', '2px');
	await expect(next).toHaveCSS('outline-style', 'solid');
	await shot(page, 'agents-views-keyboard-focus', { context: nav });
	await page.keyboard.press('Enter');
	await expect(page).toHaveURL(/\/org-chart$/);
	await expect(nav.locator('[aria-current="page"]')).toHaveText('Org chart');
	await nav.getByRole('link', { name: 'Org chart', exact: true }).focus();
	const tabKey = browserName === 'webkit' && process.platform === 'darwin' ? 'Alt+Tab' : 'Tab';
	await page.keyboard.press(tabKey);
	await expect(page.getByRole('searchbox', { name: 'Search members', exact: true })).toBeFocused();
	await page.keyboard.press(tabKey);
	await expect(page.getByRole('tab', { name: 'Teams', exact: true })).toBeFocused();
	await shot(page, 'agents-views-native-tab-order');
});

test('TEAM-11 BUG-70 S2: inner chart views, selected member and BUG-2 toolbar keep their behavior', { tag: '@webkit' }, async ({ page }) => {
	await openChart(page, 1280, 1, '?member=noor');
	const nav = page.getByRole('navigation', { name: 'Agents views', exact: true });
	const tabs = page.getByRole('tablist', { name: 'Org chart views', exact: true });
	await expect(tabs.getByRole('tab')).toHaveText(['Teams', 'People', 'Reporting lines']);
	await expect(page.getByTestId('selected-member')).toContainText('Noor Haddad');
	await tabs.getByRole('tab', { name: 'People', exact: true }).click();
	await expect(page.getByRole('tabpanel', { name: 'People', exact: true })).toBeVisible();
	await tabs.getByRole('tab', { name: 'People', exact: true }).press('ArrowRight');
	await expect(tabs.getByRole('tab', { name: 'Reporting lines', exact: true })).toHaveAttribute('aria-selected', 'true');
	await expect(page.getByTestId('selected-member')).toContainText('Noor Haddad');
	await tabs.getByRole('tab', { name: 'Teams', exact: true }).click();
	const panel = page.getByRole('tabpanel', { name: 'Teams', exact: true });
	const actions = panel.getByRole('button', { name: 'New team', exact: true }).locator('..');
	await expect(actions.getByRole('button')).toHaveText(['New team', 'Add person', 'Add agent', 'View']);
	await viewAction(page, 'Actual size');
	await expectViewAction(page, 'Fit to width');
	await expect(nav.locator('[aria-current="page"]')).toHaveText('Org chart');
	await expect(page).toHaveURL(/\/org-chart\?member=noor$/);
	await shot(page, 'agents-views-inner-tabs-toolbar', { context: tabs });
});

for (const width of [320, 640, 960, 1280]) for (const scale of [1, 2]) {
	test(`AGENT-3 TEAM-11 BUG-70 S2: nav is readable on both loaded and empty pages at ${width}px and ${scale * 100}% text`, { tag: '@webkit' }, async ({ page }) => {
		await openChart(page, width, scale);
		for (const view of [
			{ path: '/agents', current: 'Agents', empty: false },
			{ path: '/org-chart?member=noor', current: 'Org chart', empty: false },
			{ path: '/agents?org=empty', current: 'Agents', empty: true },
			{ path: '/org-chart?org=empty', current: 'Org chart', empty: true }
		]) {
			await visit(page, view.path);
			await page.addStyleTag({ content: `html { font-size: ${scale * 100}% !important; }` });
			await page.evaluate(() => document.fonts.ready.then(() => undefined));
			const nav = page.getByRole('navigation', { name: 'Agents views', exact: true });
			await expect(nav.locator('[aria-current="page"]')).toHaveText(view.current);
			await expect(nav.getByRole('link')).toHaveText(['Agents', 'Org chart']);
			const dimensions = await nav.evaluate((element) => {
				const box = element.getBoundingClientRect();
				return { left: box.left, right: box.right, fits: element.scrollWidth <= element.clientWidth,
					labelsFit: [...element.querySelectorAll('a')].every((link) => {
						const r = link.getBoundingClientRect(), range = document.createRange();
						range.selectNodeContents(link);
						return [...range.getClientRects()].every((text) => text.left >= r.left - 1 && text.right <= r.right + 1);
					}),
					padding: parseFloat(getComputedStyle(element).paddingLeft),
					underHeading: !!(document.getElementById('shell-page-title')!.compareDocumentPosition(element) & Node.DOCUMENT_POSITION_FOLLOWING)
				};
			});
			await expect(nav).toHaveCSS('overflow-x', 'auto');
			expect(dimensions.left).toBeGreaterThanOrEqual(0);
			expect(dimensions.right).toBeLessThanOrEqual(width);
			expect(dimensions.labelsFit).toBe(true);
			expect(dimensions.padding).toBe((width >= 1024 ? 32 : width >= 640 ? 24 : 16) * scale);
			expect(dimensions.underHeading).toBe(true);
			const selected = nav.locator('[aria-current="page"]');
			await expect(selected).toHaveCSS('border-bottom-width', '2px');
			const tones = await selected.evaluate((element) => {
				const probe = document.createElement('span');probe.style.color = 'var(--color-indigo-400)';element.append(probe);
				const expected = getComputedStyle(probe).color;probe.remove();
				return { expected, actual: getComputedStyle(element).borderBottomColor };
			});
			expect(tones.actual).toBe(tones.expected);
			await expect(selected).toHaveCSS('color', 'rgb(255, 255, 255)');
			if (view.empty) await expect(page.getByText(view.current === 'Agents' ? 'No agents yet' : 'No teams yet', { exact: true })).toBeVisible();
			await shot(page, `agents-views-${width}-${scale}-${view.current.replace(' ', '-').toLowerCase()}-${view.empty ? 'empty' : 'loaded'}`, { context: nav });
			for (const link of await nav.getByRole('link').all()) {
				await link.focus();
				await expect(link).toBeFocused();
				await expect.poll(() => link.evaluate((element) => {
					const box = element.getBoundingClientRect();return box.left >= 0 && box.right <= innerWidth;
				})).toBe(true);
			}
			if (!dimensions.fits) await shot(page, `agents-views-${width}-${scale}-${view.current.replace(' ', '-')}-${view.empty ? 'empty' : 'loaded'}-focused-end`, { context: nav });
		}
	});
}

for (const path of ['/agents', '/org-chart']) {
	test(`AGENT-3 TEAM-11 BUG-70 S2: ${path} keeps its loading/error boundary and recovers the nav`, { tag: '@webkit' }, async ({ page }) => {
		await openChart(page, 1280);
		const response = await page.request.get('/e2e/fakes/org-api.ts');
		expect(response.ok()).toBe(true);
		const module = await response.text();
		let entered!: () => void, release!: () => void;
		const started = new Promise<void>((resolve) => { entered = resolve; });
		const held = new Promise<void>((resolve) => { release = resolve; });
		await page.exposeFunction('__s2Entered', entered);
		await page.exposeFunction('__s2Release', () => held);
		await page.route('**/e2e/fakes/org-api.ts*', (route) => route.fulfill({ contentType: 'text/javascript', body: module + '\norgApi.view = async () => { await window.__s2Entered(); await window.__s2Release(); throw { source:"api", code:"storage_error", message:"S2 office unavailable" }; };' }));
		try {
			await page.goto(path);
			await started;
			await expect(page.getByRole('navigation', { name: 'Agents views', exact: true })).toHaveCount(0);
			await shot(page, 'agents-views-' + path.slice(1) + '-loading');
			release();
			await expect(page.getByText('S2 office unavailable', { exact: true })).toBeVisible();
			await expect(page.getByRole('navigation', { name: 'Agents views', exact: true })).toHaveCount(0);
			await shot(page, 'agents-views-' + path.slice(1) + '-error');
		} finally {
			release();
			await page.unrouteAll({ behavior: 'wait' });
		}
		await page.reload();
		await expect(page.getByRole('navigation', { name: 'Agents views', exact: true })).toBeVisible();
		await shot(page, 'agents-views-' + path.slice(1) + '-recovered');
	});
}

for (const text of [1, 2]) {
	test(`PEOPLE-1 BUG-70 BUG-90: People Fit keeps name and role lines readable at 320px and ${text * 100}% text`, { tag: '@webkit' }, async ({ page }) => {
		await openChart(page, 320, text);
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

for (const text of [1, 2]) {
	test(`PEOPLE-1 BUG-70 BUG-90: People Fit keeps collapse-control text readable without leads at ${text * 100}% text`, { tag: '@webkit' }, async ({ page }) => {
		await openChart(page, 320, text);
		const response = await page.request.get('/e2e/fakes/org-api.ts');
		expect(response.ok()).toBe(true);
		const module = await response.text();
		await page.route('**/e2e/fakes/org-api.ts*', (route) => route.fulfill({ contentType: 'text/javascript', body: module + `
			const oc1View = orgApi.view.bind(orgApi);
			orgApi.view = async (...args) => {
				const view = await oc1View(...args), snapshot = view.snapshot;
				const person = snapshot.members.find((member) => member.kind === 'person' && !member.owner);
				return { ...view, snapshot: { ...snapshot,
					teams: snapshot.teams.map((team) => ({ ...team, lead_member_id: undefined })),
					members: [...snapshot.members.filter((member) => member.owner), ...snapshot.teams.map((team) => ({ ...person, id: 'oc1-person-' + team.id, name: 'Person for ' + team.name }))],
					memberships: snapshot.teams.map((team) => ({ id: 'oc1-membership-' + team.id, team_id: team.id, member_id: 'oc1-person-' + team.id, version: 1 }))
				} };
			};` }));
		await visit(page, '/org-chart');
		await page.addStyleTag({ content: `html { font-size: ${text * 100}% !important; }` });
		await page.getByRole('tab', { name: 'People', exact: true }).click();
		const chart = page.getByTestId('people-chart');
		await expect(chart.getByTestId('people-card')).toHaveCount(5);
		await expect(chart.getByText('No people', { exact: true })).toHaveCount(0);
		await expect(chart.getByText(/^Lead:/)).toHaveCount(0);
		await expect(chart.getByText(/^\+\d+ teams?$/)).toHaveCount(0);
		const controls = chart.getByRole('button', { name: /teams?: (collapse|expand) the teams under/ });
		await expect(controls).toHaveCount(1);
		await expect.poll(() => controls.evaluateAll((elements) => Math.min(...elements.map((e) => parseFloat(getComputedStyle(e).fontSize) * new DOMMatrix(getComputedStyle(e.closest('[data-testid=people-chart]')!).transform).a)))).toBeGreaterThanOrEqual(12);
		await expect(controls.first()).toHaveAttribute('data-fit-text', '');
		await expect.poll(() => chart.evaluate((e) => new DOMMatrix(getComputedStyle(e).transform).a)).toBe(1 / text);
		await shot(page, `oc1-readable-collapse-card-${text}`, { context: controls.first().locator('..') });
		await page.getByRole('button', { name: 'View', exact: true }).press('Enter');
		await expect(page.getByRole('menuitem', { name: text === 1 ? 'Fit to width' : 'Actual size', exact: true })).toHaveAttribute('aria-disabled', text === 1 ? 'true' : 'false');
		await shot(page, `oc1-collapse-control-readable-${text}`);
	});
}

for (const text of [1, 2]) {
	test(`PEOPLE-1 BUG-70 BUG-90: membership refresh remeasures Fit when chart dimensions stay unchanged at ${text * 100}% text`, { tag: '@webkit' }, async ({ page }) => {
		await page.setViewportSize({ width: 320, height: 640 });
		await visit(page, '/create');
		await page.getByLabel('Organization name').fill('Acme');
		await page.getByRole('button', { name: 'Create organization' }).click();
		await expect(page).toHaveURL(/\/agents$/);
		const fixture = await page.evaluate(async () => {
			const connections = JSON.parse(localStorage.getItem('agentic-org.mock.connections') ?? '[]');
			const id = connections[connections.length - 1].organization.id;
			const modulePath = '/e2e/fakes/org-api.ts';
			const { orgApi } = await import(modulePath);
			const { snapshot } = await orgApi.view(id) as { snapshot: OrgSnapshot };
			const people = snapshot.members.filter((member) => member.kind === 'person' && !member.owner);
			localStorage.setItem('agentic-org.mock.org.' + id, JSON.stringify({ snapshot: { ...snapshot,
				teams: [{ id: 'small', name: 'Small', version: 1 }, { id: 'large', name: 'Large', version: 1 }],
				memberships: [
					{ id: 'small-first', team_id: 'small', member_id: people[0].id, version: 1 },
					{ id: 'large-first', team_id: 'large', member_id: people[1].id, version: 1 },
					{ id: 'large-second', team_id: 'large', member_id: people[2].id, version: 1 }
				]
			}, answers: {}, order: [] }));
			return { organizationId: id, memberId: people[1].id };
		});
		await visit(page, '/org-chart');
		await page.addStyleTag({ content: `html { font-size: ${text * 100}% !important; }` });
		await page.getByRole('tab', { name: 'People', exact: true }).click();
		const chart = page.getByTestId('people-chart');
		await expect(chart.getByTestId('people-card')).toHaveCount(2);
		await expect.poll(() => chart.evaluate((e) => new DOMMatrix(getComputedStyle(e).transform).a)).toBeCloseTo(Math.max(.45, 12 / (14 * text)), 5);
		const before = await chart.evaluate((e) => ({ width: e.offsetWidth, height: e.offsetHeight }));
		await expect(chart.getByText('+1 team', { exact: true })).toHaveCount(0);
		await page.evaluate(async ({ organizationId, memberId }) => {
			await (window as unknown as { __mockOrgChange: (organizationId: string, command: object) => Promise<unknown> }).__mockOrgChange(organizationId, { method: 'team.add_member', params: { membership_id: 'same-size-shared', team_id: 'small', member_id: memberId } });
		}, fixture);
		const badges = chart.getByText('+1 team', { exact: true });
		await expect(badges).toHaveCount(2);
		await expect.poll(() => chart.evaluate((e) => ({ width: e.offsetWidth, height: e.offsetHeight }))).toEqual(before);
		console.log('Same-dimension membership refresh', text, before, await badges.first().evaluate((e) => ({ font: getComputedStyle(e).fontSize, scale: new DOMMatrix(getComputedStyle(e.closest('[data-testid=people-chart]')!).transform).a })));
		await shot(page, `oc1-same-size-membership-${text}`, { context: chart.getByTestId('people-card').filter({ has: page.getByRole('heading', { name: 'Small', exact: true }) }) });
		await expect.poll(() => badges.evaluateAll((elements) => Math.min(...elements.map((e) => parseFloat(getComputedStyle(e).fontSize) * new DOMMatrix(getComputedStyle(e.closest('[data-testid=people-chart]')!).transform).a)))).toBeGreaterThanOrEqual(12);
		await expect.poll(() => chart.evaluate((e) => new DOMMatrix(getComputedStyle(e).transform).a)).toBe(1 / text);
		await page.getByRole('button', { name: 'View', exact: true }).press('Enter');
		await expect(page.getByRole('menuitem', { name: text === 1 ? 'Fit to width' : 'Actual size', exact: true })).toHaveAttribute('aria-disabled', text === 1 ? 'true' : 'false');
	});
}

for (const text of [1, 2]) {
	test(`PEOPLE-1 BUG-70 BUG-90: collapse and expand remeasure content at unchanged dimensions with ${text * 100}% text`, { tag: '@webkit' }, async ({ page }) => {
		await page.setViewportSize({ width: 320, height: 640 });
		await visit(page, '/create');
		await page.getByLabel('Organization name').fill('Acme');
		await page.getByRole('button', { name: 'Create organization' }).click();
		await expect(page).toHaveURL(/\/agents$/);
		await page.evaluate(async () => {
			const connections = JSON.parse(localStorage.getItem('agentic-org.mock.connections') ?? '[]');
			const id = connections[connections.length - 1].organization.id;
			const modulePath = '/e2e/fakes/org-api.ts';
			const { orgApi } = await import(modulePath);
			const { snapshot } = await orgApi.view(id) as { snapshot: OrgSnapshot };
			const person = snapshot.members.find((member) => member.kind === 'person' && !member.owner);
			const assignment = ['anchor', 'anchor', 'anchor-child', 'anchor-child', 'changing', 'changing-child'];
			localStorage.setItem('agentic-org.mock.org.' + id, JSON.stringify({ snapshot: { ...snapshot,
				teams: [
					{ id: 'anchor', name: 'Anchor', version: 1 },
					{ id: 'anchor-child', name: 'Anchor child', owner_team_id: 'anchor', version: 1 },
					{ id: 'changing', name: 'Changing', version: 1 },
					{ id: 'changing-child', name: 'Changing child', owner_team_id: 'changing', version: 1 }
				],
				members: [...snapshot.members.filter((member) => member.owner), ...assignment.map((_, index) => ({ ...person, id: 'collapse-person-' + index, name: 'Person ' + index }))],
				memberships: assignment.map((team_id, index) => ({ id: 'collapse-membership-' + index, team_id, member_id: 'collapse-person-' + index, version: 1 }))
			}, answers: {}, order: [] }));
		});
		await visit(page, '/org-chart');
		await page.addStyleTag({ content: `html { font-size: ${text * 100}% !important; }` });
		await page.getByRole('tab', { name: 'People', exact: true }).click();
		const chart = page.getByTestId('people-chart');
		await expect(chart.getByTestId('people-card')).toHaveCount(4);
		await expect.poll(() => chart.evaluate((e) => new DOMMatrix(getComputedStyle(e).transform).a)).toBe(1 / text);
		// Hold the expanded width so child-list padding cannot turn this into a resize test.
		await chart.evaluate((e) => { e.style.minWidth = `${e.offsetWidth}px`; });
		await page.evaluate(() => new Promise<void>((resolve) => requestAnimationFrame(() => requestAnimationFrame(() => resolve()))));
		const before = await chart.evaluate((e) => ({ width: e.offsetWidth, height: e.offsetHeight }));
		await chart.evaluate((element) => {
			const query = element.querySelectorAll.bind(element);
			element.dataset.measurements = '0';
			element.querySelectorAll = ((selector: string) => {
				if (selector === '[data-fit-text]') element.dataset.measurements = String(Number(element.dataset.measurements) + 1);
				return query(selector);
			}) as typeof element.querySelectorAll;
		});
		const changing = chart.getByTestId('people-card').filter({ has: page.getByRole('heading', { name: 'Changing', exact: true }) });
		for (const action of ['collapse', 'expand']) {
			await chart.evaluate((e) => { e.dataset.measurements = '0'; });
			await changing.getByRole('button', { name: `1 team: ${action} the teams under Changing`, exact: true }).click();
			await expect(chart.getByTestId('people-card')).toHaveCount(action === 'collapse' ? 3 : 4);
			await expect.poll(() => chart.evaluate((e) => ({ width: e.offsetWidth, height: e.offsetHeight }))).toEqual(before);
			await expect.poll(() => chart.evaluate((e) => Number(e.dataset.measurements))).toBeGreaterThan(0);
			await shot(page, `oc1-same-size-${action}-${text}`, { context: changing });
		}
	});
}

// Hold only the People chart observer's first measurement, not a wall-clock delay.
// This deterministically puts menu focus before the blocked-Fit notification.
async function holdPeopleMeasurement(page: Page) {
	await page.evaluate(() => {
		const pending: (() => void)[] = [];
		const NativeObserver = window.ResizeObserver;
		let held = true;
		window.ResizeObserver = class extends NativeObserver {
			constructor(callback: ResizeObserverCallback) {
				super((entries, observer) => {
					if (held && entries.some(({ target }) => target.matches('[data-testid="people-chart"]') || target.querySelector('[data-testid="people-chart"]'))) {
						pending.push(() => callback(entries, observer));
					} else callback(entries, observer);
				});
			}
		};
		Object.assign(window, { peopleMeasurementCount: () => pending.length, releasePeopleMeasurement() {
			held = false;
			const count = pending.length;
			pending.splice(0).forEach((release) => release());
			return count;
		} });
	});
}

for (const focused of ['Fit', 'Expand all'] as const) {
	test(`PEOPLE-1 BUG-70 BUG-90 BUG-123: late Fit refusal while ${focused} has keyboard focus`, { tag: '@webkit' }, async ({ page }) => {
		await openChart(page, 320);
		await holdPeopleMeasurement(page);
		await page.getByRole('tab', { name: 'People', exact: true }).click();
		await expect.poll(() => page.evaluate(() => (window as unknown as { peopleMeasurementCount(): number }).peopleMeasurementCount())).toBeGreaterThan(0);
		const trigger = page.getByRole('button', { name: 'View', exact: true });
		await trigger.press('Enter');
		const item = page.getByRole('menuitem').first();
		await expect(item).toBeFocused();
		await expect(item).not.toHaveAttribute('aria-disabled', 'true');
		const id = await item.getAttribute('id');
		if (focused === 'Expand all') await item.press('ArrowDown');
		const focus = focused === 'Fit' ? item : page.getByRole('menuitem', { name: 'Expand all', exact: true });
		await expect(focus).toBeFocused();
		const released = await page.evaluate(() => (window as unknown as { releasePeopleMeasurement(): number }).releasePeopleMeasurement());
		expect(released, 'the real People observer notification was held').toBeGreaterThan(0);
		const fit = page.getByRole('menuitem', { name: 'Fit to width', exact: true });
		await expect(fit).toHaveAttribute('aria-disabled', 'true');
		await expect(fit).toHaveAttribute('id', id!);
		await expect(focus).toBeFocused();
		await shot(page, `bug123-late-refusal-${focused.replaceAll(' ', '-')}`);
		if (focused === 'Expand all') {
			await expect(page.getByRole('tooltip')).toHaveCount(0);
			await focus.press('ArrowUp');
		}
		await expect(fit).toBeFocused();
		await expect(page.getByRole('tooltip')).toHaveText('The chart is already at its smallest readable size. Scroll to see the rest.');
		await expect(fit).toHaveAccessibleDescription('The chart is already at its smallest readable size. Scroll to see the rest.');
		await fit.press('ArrowDown');
		await expect(page.getByRole('menuitem', { name: 'Expand all', exact: true })).toBeFocused();
		await page.keyboard.press('ArrowUp');
		await expect(fit).toBeFocused();
		await expect(page.getByRole('tooltip')).toBeVisible();
		await shot(page, `bug123-focused-refusal-${focused.replaceAll(' ', '-')}`);
		await page.keyboard.press('Escape');
		await expect(page.getByRole('tooltip')).toHaveCount(0);
		await expect(fit).toBeFocused();
		await expect(page.getByRole('menu')).toBeVisible();
		await page.keyboard.press('Escape');
		await expect(page.getByRole('menu')).toHaveCount(0);
		await expect(trigger).toBeFocused();
	});
}

test('PEOPLE-1 BUG-70 BUG-90 BUG-123: removing a focused Fit refusal dismisses its explanation and preserves selection', { tag: '@webkit' }, async ({ page }) => {
	await openChart(page, 320);
	await page.getByRole('tab', { name: 'People', exact: true }).click();
	const trigger = page.getByRole('button', { name: 'View', exact: true });
	await trigger.press('Enter');
	const fit = page.getByRole('menuitem').first();
	await expect(fit).toHaveAttribute('aria-disabled', 'true');
	await expect(fit).toBeFocused();
	await expect(page.getByRole('tooltip')).toBeVisible();
	const id = await fit.getAttribute('id');
	await page.setViewportSize({ width: 2560, height: 640 });
	await expect(fit).toHaveText('Actual size');
	await expect(fit).toHaveAttribute('aria-disabled', 'false');
	await expect(fit).toHaveAttribute('id', id!);
	await expect(fit).toBeFocused();
	await expect(page.getByRole('tooltip')).toHaveCount(0);
	await shot(page, 'bug123-refusal-cleared-while-focused');
	await fit.press('Enter');
	await expect(page.getByRole('menu')).toHaveCount(0);
	await expect(trigger).toBeFocused();
	await trigger.press('Enter');
	await expect(fit).toHaveText('Fit to width');
	await expect(fit).toHaveAttribute('aria-disabled', 'false');
	await expect(page.getByRole('tooltip')).toHaveCount(0);
	await fit.press('Enter');
	await expect(page.getByRole('menu')).toHaveCount(0);
	await expect(trigger).toBeFocused();
});

test('PEOPLE-1 BUG-70 BUG-90 BUG-123: pointer Fit refusal explains without activating and dismisses outside', { tag: '@webkit' }, async ({ page }) => {
	await openChart(page, 320);
	await page.getByRole('tab', { name: 'People', exact: true }).click();
	const heading = await page.getByRole('heading', { name: 'Org chart', exact: true }).boundingBox();
	expect(heading).not.toBeNull();
	await page.getByRole('button', { name: 'View', exact: true }).click();
	const fit = page.getByRole('menuitem', { name: 'Fit to width', exact: true });
	await expect(fit).toHaveAttribute('aria-disabled', 'true');
	await page.getByRole('menuitem', { name: 'Expand all', exact: true }).hover();
	await fit.hover();
	await expect(page.getByRole('tooltip')).toBeVisible();
	await shot(page, 'bug123-pointer-refusal');
	const box = await fit.boundingBox();
	expect(box).not.toBeNull();
	// aria-disabled intentionally blocks locator.click; send a real pointer at its hit target.
	await page.mouse.click(box!.x + box!.width / 2, box!.y + box!.height / 2);
	await expect(page.getByRole('menu')).toBeVisible();
	await expect(fit).toHaveAttribute('aria-disabled', 'true');
	await expect(page.getByTestId('people-chart')).toHaveCSS('transform', 'matrix(1, 0, 0, 1, 0, 0)');
	await page.getByRole('menuitem', { name: 'Expand all', exact: true }).hover();
	await fit.hover();
	await expect(page.getByRole('tooltip')).toBeVisible();
	await page.mouse.click(heading!.x + heading!.width / 2, heading!.y + heading!.height / 2);
	await expect(page.getByRole('menu')).toHaveCount(0);
	await expect(page.getByRole('tooltip')).toHaveCount(0);
	await shot(page, 'bug123-pointer-dismissed');
});

test('PEOPLE-1 BUG-70 BUG-90 BUG-123: reopened blocked menu dismisses the explanation before the parent', { tag: '@webkit' }, async ({ page }) => {
	// Start in the default mobile List; opening Teams View first primes a different layer lifetime.
	await page.setViewportSize({ width: 320, height: 640 });
	await visit(page, '/create');
	await page.getByLabel('Organization name').fill('Acme');
	await page.getByRole('button', { name: 'Create organization' }).click();
	await expect(page).toHaveURL(/\/agents$/);
	await visit(page, '/org-chart');
	await page.addStyleTag({ content: 'html { font-size: 100% !important; }' });
	await page.evaluate(() => document.fonts.ready.then(() => undefined));
	await page.getByRole('tab', { name: 'People', exact: true }).click();
	const trigger = page.getByRole('button', { name: 'View', exact: true });
	for (const opening of ['first', 'reopened']) {
		if (opening === 'reopened') {
			// The enabled wide-menu mount followed by a blocked narrow mount exercises
			// the parent/tooltip registration order seen in the original :298 failure.
			await page.setViewportSize({ width: 2560, height: 640 });
			await trigger.click();
			await page.getByRole('menuitem', { name: 'Actual size', exact: true }).click();
			await page.setViewportSize({ width: 320, height: 640 });
		}
		await trigger.press('Enter');
		const fit = page.getByRole('menuitem', { name: 'Fit to width', exact: true });
		await expect(fit).toHaveAttribute('aria-disabled', 'true');
		await expect(fit).toBeFocused();
		await expect(page.getByRole('tooltip')).toBeVisible();
		await shot(page, `bug123-${opening}-before-escape`);
		await page.keyboard.press('Escape');
		await expect(page.getByRole('tooltip')).toHaveCount(0);
		await expect(fit).toBeFocused();
		await expect(page.getByRole('menu')).toBeVisible();
		await shot(page, `bug123-${opening}-after-first-escape`);
		await page.keyboard.press('Escape');
		await expect(page.getByRole('menu')).toHaveCount(0);
		await expect(trigger).toBeFocused();
	}
});

for (const width of [320, 390, 960, 1440]) for (const root of [20, 40]) {
	test(`PEOPLE-1 BUG-70 BUG-129: closing View rejects stale item focus at ${width}px root${root}`, { tag: '@webkit' }, async ({ page }) => {
		await openChart(page, width);
		await page.addStyleTag({ content: `html { font-size: ${root}px !important; }` });
		await expect(page.locator('html')).toHaveCSS('font-size', `${root}px`);
		await page.getByRole('tab', { name: 'People', exact: true }).click();
		const trigger = page.getByRole('button', { name: 'View', exact: true });
		await trigger.press('Enter');
		const item = page.getByRole('menuitem').first();
		await expect(item).toBeFocused({ timeout: 5_000 });
		await expect(item).toHaveAttribute('aria-disabled', 'false');
		await expect(page.getByRole('tooltip')).toHaveCount(0);
		await expect(page.getByRole('menu')).not.toHaveAttribute('inert');
		const text = await item.evaluate((element) => {
			const range = document.createRange(); range.selectNodeContents(element);
			const style = getComputedStyle(element), canvas = document.createElement('canvas').getContext('2d')!;
			canvas.font = `${style.fontSize} ${style.fontFamily}`;
			return { width: range.getBoundingClientRect().width, minimum: Math.min(canvas.measureText('000000').width, canvas.measureText(element.textContent ?? '').width) };
		});
		expect(text.width).toBeGreaterThan(0);
		expect(text.width).toBeGreaterThanOrEqual(text.minimum - 1);
		await shot(page, `bug129-enabled-view-${width}-${root}`);
		await item.evaluate((element) => {
			const trigger = document.querySelector<HTMLButtonElement>('button[aria-controls="' + element.closest('[role="menu"]')!.id + '"]')!;
			// Replay the observed late focus after default close-autofocus, within the real exit animation.
			trigger.addEventListener('focus', () => queueMicrotask(() => {
				const menu = element.closest<HTMLElement>('[role="menu"]')!;
				const before = document.activeElement === trigger;
				const closed = menu.dataset.state === 'closed';
				const connected = element.isConnected;
				(element as HTMLElement).focus();
				Object.assign(window, { bug129Attempt: { before, closed, connected, after: document.activeElement === trigger } });
			}), { once: true });
		});
		await page.keyboard.press('Escape');
		const attempt = () => page.evaluate(() => (window as unknown as { bug129Attempt?: { before: boolean; closed: boolean; connected: boolean; after: boolean } }).bug129Attempt);
		await expect.poll(attempt, { timeout: 5_000 }).toBeDefined();
		expect(await attempt(), 'stale focus must not leave View during its closing animation').toEqual({ before: true, closed: true, connected: true, after: true });
		// Inert content is hidden from default role queries before its exit animation unmounts it.
		await expect(page.getByRole('menu', { includeHidden: true })).toHaveCount(0);
		await expect(trigger).toBeFocused({ timeout: 5_000 });
		await shot(page, `bug129-escape-view-${width}-${root}`);
		// Opening again must remove the closed-state focus guard.
		await trigger.press('Enter');
		await expect(item).toBeFocused({ timeout: 5_000 });
		await expect(page.getByRole('menu')).not.toHaveAttribute('inert');
		await item.press('ArrowDown');
		await expect(page.getByRole('menuitem', { name: 'Expand all', exact: true })).toBeFocused({ timeout: 5_000 });
		await page.keyboard.press('Escape');
		await expect(page.getByRole('menu', { includeHidden: true })).toHaveCount(0);
		await expect(trigger).toBeFocused({ timeout: 5_000 });
	});
}

for (const view of ['Teams', 'People', 'Reporting lines']) {
	test(`PEOPLE-1 TEAM-1 REPORT-2 BUG-70 BUG-129: ${view} keeps first-arrow, selection and Tab focus`, { tag: '@webkit' }, async ({ page }) => {
		await openChart(page, 1280);
		await page.getByRole('tab', { name: view, exact: true }).click();
		const trigger = page.getByRole('button', { name: 'View', exact: true });
		await trigger.click();
		// No intervening wait for inert to clear: the first key after pointer-open must work.
		await page.keyboard.press('ArrowDown');
		await expect(page.getByRole('menuitem').first()).toBeFocused({ timeout: 5_000 });
		await expect(page.getByRole('menu')).not.toHaveAttribute('inert');
		await shot(page, `bug129-${view.replaceAll(' ', '-')}-first-arrow`);
		await page.getByRole('menuitem', { name: 'Collapse all', exact: true }).click();
		await expect(page.getByRole('menu', { includeHidden: true })).toHaveCount(0);
		await expect(trigger).toBeFocused({ timeout: 5_000 });
		await trigger.press('ArrowDown');
		await expect(page.getByRole('menuitem').first()).toBeFocused({ timeout: 5_000 });
		await page.keyboard.press('Tab');
		await expect(page.getByRole('menu', { includeHidden: true })).toHaveCount(0);
		await expect(page.getByRole('region', { name: view === 'Reporting lines' ? 'Reporting chart' : `${view} chart`, exact: true })).toBeFocused({ timeout: 5_000 });
		await shot(page, `bug129-${view.replaceAll(' ', '-')}-tab-focus`);
	});
}

test('PEOPLE-1 BUG-70 BUG-129: blocked Fit keeps the first Escape inside the open menu', { tag: '@webkit' }, async ({ page }) => {
	await openChart(page, 320);
	await page.getByRole('tab', { name: 'People', exact: true }).click();
	const trigger = page.getByRole('button', { name: 'View', exact: true });
	await trigger.press('Enter');
	const fit = page.getByRole('menuitem', { name: 'Fit to width', exact: true });
	await expect(fit).toHaveAttribute('aria-disabled', 'true');
	await expect(fit).toBeFocused({ timeout: 5_000 });
	await expect(page.getByRole('tooltip')).toBeVisible();
	await page.keyboard.press('Escape');
	await expect(page.getByRole('tooltip')).toHaveCount(0);
	await expect(page.getByRole('menu')).toBeVisible();
	await expect(page.getByRole('menu')).not.toHaveAttribute('inert');
	await expect(fit).toBeFocused({ timeout: 5_000 });
	await shot(page, 'bug129-first-escape-keeps-fit');
	await page.keyboard.press('Escape');
	await expect(page.getByRole('menu', { includeHidden: true })).toHaveCount(0);
	await expect(trigger).toBeFocused({ timeout: 5_000 });
	await shot(page, 'bug129-second-escape-returns-view');
});

test('PEOPLE-1 BUG-70 BUG-129: live team removal remounts View closed with a fresh focus guard', { tag: '@webkit' }, async ({ page }) => {
	await openChart(page, 1280);
	await page.getByRole('tab', { name: 'People', exact: true }).click();
	const trigger = page.getByRole('button', { name: 'View', exact: true });
	await trigger.press('Enter');
	await expect(page.getByRole('menuitem').first()).toBeFocused({ timeout: 5_000 });
	const organizationId = await page.evaluate(async () => {
		const connections = JSON.parse(localStorage.getItem('agentic-org.mock.connections') ?? '[]');
		const id = connections[connections.length - 1].organization.id;
		const modulePath = '/e2e/fakes/org-api.ts';
		const { orgApi } = await import(modulePath);
		// Use the existing command feed and real versions; parents cannot be deleted before their children.
		const { snapshot: initial } = await orgApi.view(id) as { snapshot: OrgSnapshot };
		for (let remaining = initial.teams.length; remaining > 0; remaining--) {
			const { snapshot } = await orgApi.view(id) as { snapshot: OrgSnapshot };
			const leaf = snapshot.teams.find((team) => !snapshot.teams.some((child) => child.owner_team_id === team.id))!;
			await (window as unknown as { __mockOrgChange: (id: string, command: object) => Promise<unknown> }).__mockOrgChange(id, { method: 'team.delete', params: { team_id: leaf.id, expected_version: leaf.version } });
		}
		return id;
	});
	await expect(trigger).toHaveCount(0);
	await expect(page.getByRole('menu', { includeHidden: true })).toHaveCount(0);
	await shot(page, 'bug129-live-empty-no-view');
	await page.evaluate(async (id) => {
		await (window as unknown as { __mockOrgChange: (id: string, command: object) => Promise<unknown> }).__mockOrgChange(id, { method: 'team.create', params: { team_id: 'restored-team', name: 'Restored team' } });
	}, organizationId);
	await expect(trigger).toBeVisible();
	await expect(trigger).toHaveAttribute('aria-expanded', 'false');
	await expect(page.getByRole('menu', { includeHidden: true })).toHaveCount(0);
	await shot(page, 'bug129-live-remounted-closed');
	await trigger.press('Enter');
	const item = page.getByRole('menuitem').first();
	await expect(item).toBeFocused({ timeout: 5_000 });
	await expect(page.getByRole('menu')).not.toHaveAttribute('inert');
	await item.evaluate((element) => {
		const menu = element.closest<HTMLElement>('[role="menu"]')!;
		const trigger = document.querySelector<HTMLButtonElement>('button[aria-controls="' + menu.id + '"]')!;
		trigger.addEventListener('focus', () => queueMicrotask(() => Object.assign(window, {
			bug129RemountClose: { connected: menu.isConnected, closed: menu.dataset.state === 'closed', inert: menu.inert }
		})), { once: true });
	});
	await page.keyboard.press('Escape');
	await expect.poll(() => page.evaluate(() => (window as unknown as { bug129RemountClose?: object }).bug129RemountClose), { timeout: 5_000 }).toEqual({ connected: true, closed: true, inert: true });
	await expect(page.getByRole('menu', { includeHidden: true })).toHaveCount(0);
	await expect(trigger).toBeFocused({ timeout: 5_000 });
	await shot(page, 'bug129-live-remounted-escape');
});

// The chart owns this fixture setup; keep its target/icon oracle local to avoid importing Projects test internals.
const icons = {
	"mini:x-mark": {
		"url": "https://raw.githubusercontent.com/tailwindlabs/heroicons/v2.2.0/optimized/20/solid/x-mark.svg",
		"attributes": {
			"viewBox": "0 0 20 20",
			"fill": "currentColor",
			"aria-hidden": "true",
			"data-slot": "icon"
		},
		"paths": [
			{
				"d": "M6.28 5.22a.75.75 0 0 0-1.06 1.06L8.94 10l-3.72 3.72a.75.75 0 1 0 1.06 1.06L10 11.06l3.72 3.72a.75.75 0 1 0 1.06-1.06L11.06 10l3.72-3.72a.75.75 0 0 0-1.06-1.06L10 8.94 6.28 5.22Z"
			}
		]
	}
};

async function createChipOrg(page: Page, width: number, scale = 1) {
	await page.setViewportSize({ width, height: 900 });
	await page.addInitScript((scale) => {
		const apply = () => document.documentElement.style.setProperty('font-size', `${scale * 100}%`, 'important');
		addEventListener('DOMContentLoaded', apply, { once: true });
		if (document.documentElement) apply();
	}, scale);
	await visit(page, '/create');
	await page.getByLabel('Organization name').fill('Acme');
	await page.getByRole('button', { name: 'Create organization', exact: true }).click();
	await expect(page).toHaveURL(/\/agents$/);
	// URL changes before the destination is ready; avoid interrupting the create handoff (BUG-93).
	await expect(page.getByRole('main', { name: 'Agents', exact: true })).toBeVisible();
	await expect(page.getByRole('table', { name: 'Agents', exact: true })).toBeVisible();
}
async function chartIconIs(icon: Locator, key: keyof typeof icons) {
	const expected = icons[key];
	await expect(icon).toHaveCount(1);
	for (const [name, value] of Object.entries(expected.attributes)) await expect(icon).toHaveAttribute(name, value);
	await expect(icon).toHaveAttribute('focusable', 'false');
	expect(await icon.locator('path').evaluateAll((paths) => paths.map((path) => Object.fromEntries([...path.attributes].map((a) => [a.name, a.value]))))).toEqual(expected.paths);
	const font = await icon.evaluate(() => parseFloat(getComputedStyle(document.documentElement).fontSize));
	await expect(icon).toHaveCSS('width', `${font}px`);
	await expect(icon).toHaveCSS('height', `${font}px`);
}
async function chartRemoveIs(button: Locator, name: string) {
	await expect(button).toHaveAccessibleName(name);
	await expect(button).toHaveAttribute('title', name);
	await expect(button).toHaveText('');
	await chartIconIs(button.locator('svg'), 'mini:x-mark');
	const box = (await button.boundingBox())!;
	expect(box.width, `${name}: displayed target width`).toBeGreaterThanOrEqual(24);
	expect(box.height, `${name}: displayed target height`).toBeGreaterThanOrEqual(24);
}
async function chartFocusTarget(page: Page, button: Locator) {
	// A pointer click on Actual size leaves pointer modality; enter keyboard mode first.
	await page.keyboard.press('Tab');
	await button.evaluate((element) => element.scrollIntoView({ block: 'center', inline: 'center' }));
	await button.focus();
	await expect(button).toBeFocused();
	await expect.poll(() => button.evaluate((element) => {
		const r = element.getBoundingClientRect(), x = r.x + r.width / 2, y = r.y + r.height / 2;
		const hit = document.elementFromPoint(x, y);
		return x >= 0 && x < innerWidth && y >= 0 && y < innerHeight && !!hit && element.contains(hit);
	})).toBe(true);
}
const chipTeam = (page: Page, id: string) => page.locator(`[data-testid=team-card][data-team=${id}]`);

/** The phone defaults to List; choose Chart through the same retained View menu. */
async function showChipChart(page: Page) {
	if (page.viewportSize()!.width < 640) await viewAction(page, 'Chart');
	await expect(page.getByTestId('teams-chart')).toBeVisible();
}

async function removalRing(button: import('@playwright/test').Locator) {
	await expect(button).toHaveCSS('outline-offset', '2px');
	await expect(button).toHaveCSS('outline-style', 'solid');
	const clearances = await button.evaluate((element) => {
		const r = element.getBoundingClientRect(), values: { ancestor: string; sides: number[] }[] = [];
		for (let parent = element.parentElement; parent; parent = parent.parentElement) {
			const css = getComputedStyle(parent), box = parent.getBoundingClientRect();
			const x = /auto|scroll|hidden|clip/.test(css.overflowX), y = /auto|scroll|hidden|clip/.test(css.overflowY);
			if (x || y) values.push({ ancestor: `${parent.tagName}.${parent.className}`, sides: [
				...(x ? [r.left - box.left, box.right - r.right] : []), ...(y ? [r.top - box.top, box.bottom - r.bottom] : [])] });
		}
		return values;
	});
	for (const clip of clearances) for (const side of clip.sides) expect(side, clip.ancestor).toBeGreaterThanOrEqual(4 - 0.1);
}

async function chipFixture(page: import('@playwright/test').Page) {
	await page.evaluate(async () => {
		const connections = JSON.parse(localStorage.getItem('agentic-org.mock.connections') ?? '[]');
		const id = connections[connections.length - 1].organization.id;
		const modulePath = '/e2e/fakes/org-api.ts';
		const { orgApi } = await import(modulePath);
		const { snapshot } = await orgApi.view(id);
		const links = [['beacon', 'platform'], ['atlas', 'web'], ['scratch', 'web'], ['scratch', 'design']];
		localStorage.setItem('agentic-org.mock.org.' + id, JSON.stringify({ snapshot: { ...snapshot,
			project_teams: [...snapshot.project_teams, ...links.map(([project_id, team_id], index) => ({ id: `chip-fixture-${index}`, project_id, team_id, version: 1 }))]
		}, answers: {}, order: [] }));
	});
}

async function geometryFocus(page: import('@playwright/test').Page, button: import('@playwright/test').Locator) {
	await page.keyboard.press('Tab');
	await button.evaluate((element) => element.scrollIntoView({ block: 'center', inline: 'center' }));
	await button.focus();
	await expect(button).toBeFocused();
}

// BUG-114/115/117: exercise the displayed targets and native focus after the keyed DOM refresh.
for (const width of [320, 640, 960, 1280]) for (const root of [16, 32, 40]) {
	test(`TEAM-8 BUG-70 BUG-114 BUG-117: Fit removal target and ring at ${width}px root${root}`, { tag: '@webkit' }, async ({ page }) => {
		const errors: string[] = [];
		page.on('pageerror', (error) => errors.push(error.message));
		await createChipOrg(page, width, root / 16);
		await visit(page, '/org-chart');
		await showChipChart(page);
		const chart = page.getByTestId('teams-chart');
		const remove = chipTeam(page, 'web').getByRole('button', { name: 'Remove Beacon from Web', exact: true });
		await geometryFocus(page, remove);
		await shot(page, `bug114-fit-${width}-${root}`, { context: remove });
		await removalRing(remove);
		const samples = await chart.evaluate(async (element) => {
			const sample = () => ({ scale: new DOMMatrix(getComputedStyle(element).transform).a,
				width: element.getBoundingClientRect().width, naturalWidth: (element as HTMLElement).offsetWidth });
			const values = [sample()];
			for (let i = 0; i < 2; i++) { await new Promise(requestAnimationFrame); values.push(sample()); }
			return values;
		});
		console.log(`BUG114-FIT ${JSON.stringify({ width, root, samples, target: await remove.boundingBox() })}`);
		expect(Math.max(...samples.map((x) => x.scale)) - Math.min(...samples.map((x) => x.scale))).toBeLessThanOrEqual(0.001);
		expect(errors.filter((error) => /ResizeObserver loop/.test(error))).toEqual([]);
		const box = (await remove.boundingBox())!;
		await page.getByRole('button', { name: 'View', exact: true }).focus();
		await page.keyboard.press('Enter');
		const actual = page.getByRole('menuitem', { name: 'Actual size', exact: true });
		await actual.focus();
		await actual.press('Enter');
		await geometryFocus(page, remove);
		await shot(page, `bug114-actual-${width}-${root}`, { context: remove });
		await expect(chart).toHaveCSS('transform', 'matrix(1, 0, 0, 1, 0, 0)');
		await chartRemoveIs(remove, 'Remove Beacon from Web');
		expect((await remove.boundingBox())!.width).toBeCloseTo(root * 1.5, 1);
		expect(box.width, 'counter-scaled displayed width').toBeGreaterThanOrEqual(root * 1.5 - 0.1);
		expect(box.height, 'counter-scaled displayed height').toBeGreaterThanOrEqual(root * 1.5 - 0.1);
		await removalRing(remove);
	});
}

for (const direction of ['next', 'previous'] as const) {
	test(`TEAM-8 BUG-70 BUG-115: chart confirmed-last unlink uses the same keyed ${direction} resolver`, { tag: '@webkit' }, async ({ page }) => {
		await createChipOrg(page, 1280);
		await chipFixture(page);
		await page.evaluate(async (direction) => {
			const connections = JSON.parse(localStorage.getItem('agentic-org.mock.connections') ?? '[]');
			const id = connections[connections.length - 1].organization.id;
			const hooks = window as unknown as { __mockOrgChange(id: string, command: object): Promise<unknown> };
			await hooks.__mockOrgChange(id, { method: 'project.remove_team', params: direction === 'next'
				? { project_team_id: 'pt-1', expected_version: 1, areas_to_team_id: 'web' }
				: { project_team_id: 'chip-fixture-3', expected_version: 1 } });
		}, direction);
		await visit(page, '/org-chart');
		const name = `Remove ${direction === 'next' ? 'Atlas' : 'Scratchpad'} from Web`;
		const remove = page.getByRole('button', { name, exact: true });
		await chartFocusTarget(page, remove);
		await remove.press('Enter');
		const dialog = page.getByRole('dialog', { name: `${name}?`, exact: true });
		await expect(dialog).toBeVisible();
		await dialog.getByRole('button', { name: 'Remove', exact: true }).click();
		await expect(dialog).toBeHidden();
		await expect(remove).toHaveCount(0);
		const target = page.getByRole('button', { name: 'Remove Beacon from Web', exact: true });
		await expect(target).toBeFocused();
		await shot(page, `bug115-chart-confirmed-${direction}`, { context: target });
	});
}

test('TEAM-8 BUG-70 BUG-115: chart falls back to its region if the source card disappears during success', { tag: '@webkit' }, async ({ page }) => {
	await createChipOrg(page, 1280);
	await visit(page, '/org-chart');
	await page.evaluate(async () => {
		const modulePath = '/e2e/fakes/org-api.ts';
		const { orgApi } = await import(modulePath) as { orgApi: import('../src/lib/org/api').OrgApi };
		const original = orgApi.command;
		orgApi.command = async (id, input) => {
			const result = await original(id, input);
			if (input.method === 'project.remove_team') await original(id, { method: 'team.delete', params: { team_id: 'web', expected_version: 1 } });
			return result;
		};
	});
	const remove = page.getByRole('button', { name: 'Remove Beacon from Web', exact: true });
	await chartFocusTarget(page, remove);
	await remove.press('Enter');
	await expect(chipTeam(page, 'web')).toHaveCount(0);
	const region = page.getByRole('region', { name: 'Teams chart', exact: true });
	await expect(region).toBeFocused();
	await expect(page.getByRole('button', { name: 'Remove Beacon from Design', exact: true })).not.toBeFocused();
	await shot(page, 'bug115-chart-region-fallback', { context: region });
});

for (const root of [16, 32, 40]) {
	test(`TEAM-8 BUG-70 BUG-114: Actual size keeps the unfocused control at root${root}`, { tag: '@webkit' }, async ({ page }) => {
		await createChipOrg(page, 1280, root / 16);
		await visit(page, '/org-chart');
		await page.getByRole('button', { name: 'View', exact: true }).focus();
		await page.keyboard.press('Enter');
		const actual = page.getByRole('menuitem', { name: 'Actual size', exact: true });
		await actual.focus();
		await actual.press('Enter');
		await expect(page.getByTestId('teams-chart')).toHaveCSS('transform', 'matrix(1, 0, 0, 1, 0, 0)');
		await page.evaluate(async () => { await new Promise(requestAnimationFrame); await new Promise(requestAnimationFrame); });
		const remove = page.getByRole('button', { name: 'Remove Beacon from Web', exact: true });
		await chartRemoveIs(remove, 'Remove Beacon from Web');
		await expect(remove).not.toBeFocused();
		await shot(page, `bug114-actual-unfocused-${root}`, { context: remove });
	});
}

test('TEAM-8 BUG-70 BUG-115: chart confirmation retains its own focus plan across a later cancelled removal', { tag: '@webkit' }, async ({ page }) => {
	await createChipOrg(page, 1280);
	await visit(page, '/org-chart');
	const first = page.getByRole('button', { name: 'Remove Beacon from Web', exact: true });
	await chartFocusTarget(page, first);
	await first.press('Enter');
	await expect(first).toHaveCount(0);
	const last = page.getByRole('button', { name: 'Remove Beacon from Design', exact: true });
	await last.focus();
	await last.press('Enter');
	const oldDialog = page.getByRole('dialog', { name: 'Remove Beacon from Design?', exact: true });
	await expect(oldDialog).toBeVisible();
	await page.evaluate(async () => {
		const modulePath = '/e2e/fakes/org-api.ts';
		const { orgApi } = await import(modulePath) as { orgApi: import('../src/lib/org/api').OrgApi };
		const command = orgApi.command, view = orgApi.view;
		let pending = false, release!: () => void;
		const held = new Promise<void>((resolve) => { release = resolve; });
		const hooks = window as unknown as { __chipRefresh: { entered: boolean; release(): void } };
		hooks.__chipRefresh = { entered: false, release };
		orgApi.command = async (id, input) => {
			if (input.method === 'project.remove_team') pending = true;
			return command(id, input);
		};
		orgApi.view = async (id) => {
			if (pending) { hooks.__chipRefresh.entered = true; await held; }
			return view(id);
		};
	});
	await oldDialog.getByRole('button', { name: 'Remove', exact: true }).click();
	await expect(oldDialog).toBeHidden();
	await page.waitForFunction(() => (window as unknown as { __chipRefresh: { entered: boolean } }).__chipRefresh.entered);
	const later = page.getByRole('button', { name: 'Remove Atlas from Platform', exact: true });
	await later.focus();
	await later.press('Enter');
	const nextDialog = page.getByRole('dialog', { name: 'Remove Atlas from Platform?', exact: true });
	await expect(nextDialog).toBeVisible();
	await nextDialog.getByRole('button', { name: 'Cancel', exact: true }).click();
	await expect(nextDialog).toBeHidden();
	await expect(later).toBeFocused();
	await page.evaluate(() => (window as unknown as { __chipRefresh: { release(): void } }).__chipRefresh.release());
	await expect(last).toHaveCount(0);
	await shot(page, 'bug115-chart-confirmation-plan-race', { context: later });
	await expect(later).toBeFocused();
});
