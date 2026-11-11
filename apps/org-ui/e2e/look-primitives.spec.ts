import { expect, test, type Locator, type Page } from '@playwright/test';
import { visit } from './ready';
import { shot } from './shot';

async function open(page: Page, path = '/agents') {
	await visit(page, '/create');
	await page.getByLabel('Organization name').fill('Acme');
	await page.getByRole('button', { name: 'Create organization' }).click();
	await expect(page).toHaveURL(/\/agents$/);
	if (path !== '/agents') await visit(page, path);
}

async function tokenColor(page: Page, token: string) {
	return page.evaluate((token) => {
		const reference = document.createElement('span');
		reference.style.color = `var(${token})`;
		document.body.append(reference);
		const color = getComputedStyle(reference).color;
		reference.remove();
		return color;
	}, token);
}

async function css(locator: Locator, property: string) {
	return locator.evaluate((element, property) => getComputedStyle(element).getPropertyValue(property), property);
}

async function contrast(locator: Locator) {
	return locator.evaluate((element) => {
		// These screens use flat colors. Resolve modern CSS colors to sRGB and composite their real ancestor tints.
		const canvas = document.createElement('canvas');
		canvas.width = canvas.height = 1;
		const context = canvas.getContext('2d')!;
		const rgba = (color: string) => {
			context.clearRect(0, 0, 1, 1);
			context.fillStyle = color;
			context.fillRect(0, 0, 1, 1);
			return [...context.getImageData(0, 0, 1, 1).data];
		};
		const backgrounds = [];
		for (let node: Element | null = element; node; node = node.parentElement) backgrounds.push(rgba(getComputedStyle(node).backgroundColor));
		const over = (front: number[], back: number[]) => back.map((channel, i) => front[i] * front[3] / 255 + channel * (1 - front[3] / 255));
		const background = backgrounds.reverse().reduce((back, front) => over(front, back), [255, 255, 255]);
		const foreground = over(rgba(getComputedStyle(element).color), background);
		const luminance = (color: number[]) => color.map((c) => c / 255).map((c) => c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4).reduce((sum, c, i) => sum + c * [0.2126, 0.7152, 0.0722][i], 0);
		const a = luminance(foreground), b = luminance(background);
		return (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05);
	});
}

async function fits(locator: Locator) {
	return locator.evaluate((element) => {
		const box = element.getBoundingClientRect();
		const range = document.createRange();
		range.selectNodeContents(element);
		const rects = [...range.getClientRects()];
		return rects.length > 0 && rects.every((rect) => rect.left >= box.left - 1 && rect.right <= box.right + 1);
	});
}

for (const width of [320, 640, 960, 1280]) {
	test(`AGENT-3 BUG-70: a readable outlined status pill preserves roster and filters at ${width}px`, async ({ page }) => {
		await page.setViewportSize({ width, height: 900 });
		await open(page);
		const roster = page.getByRole('table', { name: 'Agents', exact: true });
		await expect(roster.getByTestId('agent-row')).toHaveCount(8);
		const noor = roster.locator('[data-agent=noor]');
		const status = noor.getByTestId('agent-status');
		await expect(status).toHaveText('Asleep · wakes when called');
		const style = await status.evaluate((element) => {
			const s = getComputedStyle(element);
			return { radius: parseFloat(s.borderRadius), x: parseFloat(s.paddingLeft), y: parseFloat(s.paddingTop), shadow: s.boxShadow };
		});
		expect.soft(style.radius).toBeGreaterThanOrEqual(12);
		expect.soft(style.x).toBe(8);
		expect.soft(style.y).toBe(4);
		expect.soft(style.shadow).not.toBe('none');
		expect.soft(await css(status, 'color')).toBe(await tokenColor(page, '--color-gray-300'));
		const ratio = await contrast(status);
		expect.soft(ratio).toBeGreaterThanOrEqual(4.5);
		console.info(`Status text contrast at ${width}px: ${ratio.toFixed(2)}:1`);
		await expect(status.locator('svg')).toHaveAttribute('aria-hidden', 'true');
		await expect(noor.getByRole('link', { name: 'Noor Haddad', exact: true })).toHaveAttribute('href', '/org-chart?member=noor');
		const active = page.getByRole('button', { name: /^Active / });
		await active.focus();
		await active.press('Enter');
		await expect(active).toBeFocused();
		await expect(active).toHaveAttribute('aria-pressed', 'true');
		await expect(roster.getByTestId('agent-row')).toHaveCount(8);
		await status.scrollIntoViewIfNeeded();
		await shot(page, `look-pill-${width}`, { context: status });
	});

	test(`PROJ-1 BUG-70: native divided rows and a quiet neutral callout at ${width}px`, async ({ page }) => {
		await page.setViewportSize({ width, height: 900 });
		await open(page, '/projects');
		const list = page.getByRole('list', { name: 'Projects', exact: true });
		await expect(list.getByRole('listitem')).toHaveCount(4);
		await expect(list.getByTestId('project-row').getByRole('link')).toHaveText(['Atlas', 'Beacon', 'Onboarding kit', 'Scratchpad']);
		const first = list.getByRole('listitem').first();
		expect.soft(await css(first, 'padding-top')).toBe('16px');
		expect.soft(await css(first, 'padding-bottom')).toBe('16px');
		expect.soft(await css(list, 'border-radius')).toBe('0px');
		expect.soft(await css(list, 'background-color')).toBe('rgba(0, 0, 0, 0)');
		const note = page.getByTestId('unassigned-note');
		await expect(note).toHaveText('1 project has no team yet.');
		expect.soft(await css(note, 'color')).toBe(await tokenColor(page, '--color-gray-300'));
		expect(await contrast(note)).toBeGreaterThanOrEqual(4.5);
		expect.soft(await note.locator('svg[aria-hidden=true]').count()).toBe(1);
		expect(await note.getAttribute('role')).toBeNull(); // Static context must not acquire Alert's default live role.
		expect(await note.getAttribute('aria-live')).toBeNull();
		expect(await fits(note)).toBe(true);
		await expect(page.getByRole('button', { name: 'New project', exact: true })).toBeDisabled();
		await shot(page, `look-projects-${width}`, { context: list });
	});
}

for (const width of [320, 960]) {
	test(`AGENT-3 BUG-70: status text wraps inside a constrained cell at 200% text and ${width}px`, async ({ page }) => {
		await page.setViewportSize({ width, height: 900 });
		await open(page);
		await page.addStyleTag({ content: 'html { font-size: 200%; }' });
		const table = page.getByRole('table', { name: 'Agents', exact: true });
		// Stress the real component in an 8rem cell, without pretending the existing roster table is responsive.
		await table.evaluate((element) => { element.style.tableLayout = 'fixed'; element.style.width = '64rem'; });
		const status = table.locator('[data-agent=noor]').getByTestId('agent-status');
		await expect(status).toHaveText('Asleep · wakes when called');
		await expect(status).toHaveCSS('white-space', 'normal');
		expect(await fits(status)).toBe(true);
		expect(await status.evaluate((element) => element.getBoundingClientRect().width <= element.parentElement!.clientWidth)).toBe(true);
		await status.scrollIntoViewIfNeeded();
		await shot(page, `look-pill-200-${width}`, { context: status });
	});

	test(`PROJ-1 BUG-70: a long project name and callout wrap at 200% text and ${width}px`, async ({ page }) => {
		await page.setViewportSize({ width, height: 900 });
		await open(page);
		const long = 'Project' + 'x'.repeat(64);
		await page.evaluate(async (name) => {
			const path = '/e2e/fakes/org-api.ts';
			const { orgApi } = await import(path);
			const view = orgApi.view.bind(orgApi);
			orgApi.view = async (id: string) => {
				const result = await view(id);
				return { ...result, snapshot: { ...result.snapshot, projects: result.snapshot.projects.map((p: { id: string }) => p.id === 'scratch' ? { ...p, name } : p) } };
			};
		}, long);
		if (width < 640) await page.getByRole('button', { name: 'Menu', exact: true }).click();
		await page.getByRole('link', { name: 'Projects', exact: true }).first().click();
		await expect(page).toHaveURL(/\/projects$/);
		await page.addStyleTag({ content: 'html { font-size: 200%; }' });
		const row = page.locator('[data-testid=project-row][data-project=scratch]');
		const link = row.getByRole('link', { name: long, exact: true });
		await expect(link).toBeVisible();
		expect(await fits(link)).toBe(true);
		expect(await fits(page.getByTestId('unassigned-note'))).toBe(true);
		// A compact icon column must yield before ordinary words are split into letters.
		const wordLines = await page.getByTestId('unassigned-note').evaluate((element) => {
			const walker = document.createTreeWalker(element, NodeFilter.SHOW_TEXT);
			for (let node = walker.nextNode(); node; node = walker.nextNode()) {
				const start = node.textContent!.indexOf('project');
				if (start < 0) continue;
				const range = document.createRange();
				range.setStart(node, start);
				range.setEnd(node, start + 'project'.length);
				return range.getClientRects().length;
			}
			throw new Error('The project word is missing');
		});
		expect(wordLines).toBe(1);
		expect(await css(row, 'padding-top')).toBe('32px');
		await shot(page, `look-callout-200-${width}`, { context: page.getByTestId('unassigned-note') });
		await shot(page, `look-projects-long-200-${width}`, { context: row });
	});
}

test('AGENT-3 BUG-70: Paused keeps its words, warning tint and keyboard filter', async ({ page }) => {
	await open(page, '/org-chart');
	await page.locator('[data-testid=team-card][data-team=web]').getByRole('button', { name: /^Noor Haddad/ }).click();
	await page.getByTestId('selected-member').getByRole('button', { name: 'Pause…' }).click();
	await page.getByRole('dialog').getByRole('button', { name: 'Pause Noor Haddad' }).click();
	await expect(page.getByText('Paused Noor Haddad.')).toBeVisible();
	await visit(page, '/agents');
	const filter = page.getByRole('button', { name: /^Paused / });
	await filter.focus();
	await filter.press('Enter');
	await expect(filter).toBeFocused();
	await expect(page.getByTestId('agent-row')).toHaveCount(1);
	const status = page.getByTestId('agent-status');
	await expect(status).toHaveText('Paused');
	expect(await css(status, 'color')).toBe(await tokenColor(page, '--color-amber-200'));
	expect(await contrast(status)).toBeGreaterThanOrEqual(4.5);
	expect(await css(status, 'padding-top')).toBe('4px');
	await shot(page, 'look-pill-paused', { context: status });
});

test('PROJ-2/3 BUG-70: row links and keyboard assignment keep their behavior', async ({ page }) => {
	await open(page, '/projects');
	const row = page.locator('[data-testid=project-row][data-project=scratch]');
	const assign = row.getByRole('button', { name: 'Assign a team to Scratchpad', exact: true });
	await assign.focus();
	await assign.press('Enter');
	const dialog = page.getByRole('dialog', { name: 'Assign a team to Scratchpad', exact: true });
	await dialog.getByRole('radio', { name: 'Design', exact: true }).check();
	await dialog.getByRole('button', { name: 'Assign team', exact: true }).click();
	await expect(page.getByText('Scratchpad now belongs to Design.')).toBeVisible();
	await expect(page.getByTestId('unassigned-note')).toHaveCount(0);
	await expect(row.getByTestId('project-team')).toHaveText(/Design/);
	await expect(assign).toBeFocused();
	const link = row.getByRole('link', { name: 'Scratchpad', exact: true });
	await link.focus();
	await link.press('Enter');
	await expect(page).toHaveURL(/\/projects\/scratch$/);
	await expect(page.getByTestId('project-name')).toHaveText('Scratchpad');
});

test('BUG-70: primitives keep status words, caller roles and native list attributes', async ({ page }) => {
	await open(page);
	await page.evaluate(async () => {
		const path = '/src/lib/components/testing/look-primitives.ts';
		const { mountPrimitives } = await import(path);
		const target = document.createElement('div');
		document.body.append(target);
		mountPrimitives(target);
	});
	const fixture = page.getByRole('region', { name: 'Primitive contracts', exact: true });
	await expect(fixture).toBeVisible();
	await expect(fixture.getByTestId('word-only')).toHaveText('Status unknown');
	await expect(fixture.getByTestId('word-only').locator('svg')).toHaveCount(0);
	await expect(fixture.getByTestId('word-and-icon')).toHaveText('Paused');
	await expect(fixture.getByTestId('word-and-icon').locator('svg')).toBeVisible();
	await expect(fixture.getByRole('img', { name: 'Status ornament', exact: true })).toHaveCount(0);
	await expect(fixture.getByTestId('icon-callout').locator('svg')).toBeVisible();
	await expect(fixture.getByRole('img', { name: 'Callout ornament', exact: true })).toHaveCount(0);
	const longLabel = fixture.getByTestId('long-label');
	await expect(longLabel).toHaveText('Status' + 'x'.repeat(64));
	expect(await fits(longLabel)).toBe(true);
	expect(await longLabel.evaluate((element) => element.getBoundingClientRect().width <= element.parentElement!.clientWidth)).toBe(true);
	expect(await fits(fixture.getByTestId('long-callout'))).toBe(true);
	const quiet = fixture.getByTestId('quiet-contract');
	expect(await quiet.getAttribute('role')).toBeNull();
	expect(await quiet.getAttribute('aria-live')).toBeNull();
	await expect(fixture.getByRole('status')).toHaveText('Saved progress');
	await expect(fixture.getByRole('alert')).toHaveText('Failed update');
	await expect(fixture.getByRole('status')).toHaveCount(1);
	await expect(fixture.getByRole('alert')).toHaveCount(1);
	const row = fixture.getByRole('list', { name: 'Ordered facts', exact: true }).getByRole('listitem');
	await expect(row).toHaveAttribute('value', '7');
	await expect(row).toHaveAttribute('lang', 'pt');
	await expect(row).toHaveAttribute('title', 'Native row');
	await expect(row).toHaveAttribute('data-testid', 'native-row');
	expect(await row.getAttribute('role')).toBeNull();
	expect(await row.getAttribute('tabindex')).toBeNull();
	expect(await row.getAttribute('onclick')).toBeNull();
	await shot(page, 'look-primitives-contracts', { context: fixture });
});

for (const width of [320, 640, 960, 1280]) for (const scale of [1, 2]) {
	test('AGENT-3 BUG-70: flat roster keeps all states at ' + width + 'px and ' + scale * 100 + '% text', async ({ page }) => {
		await open(page);
		await page.setViewportSize({ width, height: 900 });
		await page.locator('html').evaluate((html, pixels) => { html.style.fontSize = pixels + 'px'; }, 16 * scale);
		expect((await page.request.get('/e2e/fakes/org-api.ts')).status()).toBe(200);
		const filters = page.getByRole('group', { name: 'Show agents', exact: true });
		await filters.getByRole('button', { name: /^Former / }).press('Enter');
		await expect(page.getByTestId('no-match')).toHaveText('No left agents.');
		await expect(page.getByRole('table', { name: 'Agents', exact: true })).toHaveCount(0);

		await shot(page, 'agents-flat-' + width + '-' + scale + '-no-match', { context: page.getByTestId('no-match') });
		await page.setViewportSize({ width: 1440, height: 900 });

		// Existing user flows provide real Paused/Former rows in the test-owned org.
		await visit(page, '/org-chart');
		await page.locator('[data-testid=team-card][data-team=web]').getByRole('button', { name: /^Noor Haddad/ }).click();
		await page.getByTestId('selected-member').getByRole('button', { name: 'Pause…' }).click();
		await page.getByRole('dialog').getByRole('button', { name: 'Pause Noor Haddad' }).click();
		await expect(page.getByText('Paused Noor Haddad.')).toBeVisible();
		await page.locator('[data-testid=team-card][data-team=eng]').getByRole('button', { name: /^Sam Okafor/ }).click();
		await page.getByTestId('selected-member').getByRole('button', { name: 'Offboard…' }).click();
		await page.getByRole('dialog').getByRole('button', { name: 'Offboard Sam Okafor' }).click();
		await expect(page.getByText('Offboarded Sam Okafor.')).toBeVisible();
		await visit(page, '/agents');
		await page.setViewportSize({ width, height: 900 });
		await page.locator('html').evaluate((html, pixels) => { html.style.fontSize = pixels + 'px'; }, 16 * scale);
		const table = page.getByRole('table', { name: 'Agents', exact: true });
		const columns = ['Agent', 'Runs on', 'How it runs', 'Status', 'Now', 'Teams', 'Office', 'Reports to'];
		await expect(filters.getByRole('button')).toHaveText(['All 7', 'Active 6', 'Paused 1', 'Former 1']);
		for (const [name, count] of [['All', 7], ['Active', 6], ['Paused', 1], ['Former', 1]] as const) {
			const filter = filters.getByRole('button', { name: new RegExp('^' + name + ' ') });
			await filter.focus(); await filter.press('Enter');
			await expect(filter).toBeFocused();
			await expect(filter).toHaveAttribute('aria-pressed', 'true');
			await expect(table.getByRole('columnheader')).toHaveText(columns);
			await expect(table.getByTestId('agent-row')).toHaveCount(count);
			const wrapper = table.locator('..');
			expect.soft(await css(wrapper, 'background-color')).toBe('rgba(0, 0, 0, 0)');
			expect.soft(await css(wrapper, 'border-radius')).toBe('0px');
			expect.soft(await css(wrapper, 'border-top-width')).toBe('1px');
			expect.soft(await css(wrapper, 'border-bottom-width')).toBe('1px');
			expect.soft(await css(wrapper, 'overflow-x')).toBe('auto');
			const cells = table.locator('tbody tr').first().locator('th, td');
			await expect(cells).toHaveCount(8);
			const padding = await cells.evaluateAll((cells) => cells.map((cell) => ({
				top: parseFloat(getComputedStyle(cell).paddingTop), bottom: parseFloat(getComputedStyle(cell).paddingBottom)
			})));
			expect.soft(padding.map((p) => p.top)).toEqual(Array(8).fill(16 * scale));
			expect.soft(padding.map((p) => p.bottom)).toEqual(Array(8).fill(16 * scale));
			if (count > 1) expect.soft(await css(table.locator('tbody tr').first(), 'border-bottom-width')).toBe('1px');
			// Badge transitions include padding; observe the settled size after changing root text size.
			await expect.poll(async () => parseFloat(await css(table.getByTestId('agent-status').first(), 'padding-top'))).toBe(4 * scale);
			if (name === 'All') await expect(table.locator('[data-agent=noor] td').nth(4)).toHaveText('Design, Web');
			if (name === 'Former') await expect(cells.nth(5)).toHaveText('—');
			if (name === 'Paused') await expect(table.getByTestId('agent-status')).toHaveText('Paused');
			if (name === 'Former') await expect(table.getByTestId('agent-status')).toHaveText('Former');
			await cells.first().scrollIntoViewIfNeeded();
			await wrapper.evaluate((element) => { element.scrollLeft = 0; });
			const readableFirst = await cells.first().evaluate((cell) => {
				const box = cell.getBoundingClientRect();
				const clip = cell.closest('table')!.parentElement!.getBoundingClientRect();
				const text = document.createTreeWalker(cell, NodeFilter.SHOW_TEXT);
				const glyphs: DOMRect[] = [];
				while (text.nextNode()) {
					if (!text.currentNode.textContent?.trim()) continue;
					const range = document.createRange(); range.selectNodeContents(text.currentNode);
					glyphs.push(...range.getClientRects());
				}
				return { count: glyphs.length, fits: glyphs.every((r) => r.left >= Math.max(0, box.left, clip.left) - 1 && r.right <= Math.min(innerWidth, box.right, clip.right) + 1) };
			});
			expect.soft(readableFirst.count).toBeGreaterThan(0);
			expect.soft(readableFirst.fits).toBe(true);
			expect.soft(await css(cells.first(), 'position')).not.toBe('sticky');
			expect.soft(await css(wrapper, 'outline-style')).toBe('none');
			// Exercise the conditional focusable case without adding a production tab stop.
			const previousTabindex = await wrapper.getAttribute('tabindex');
			await wrapper.evaluate((element) => element.setAttribute('tabindex', '0'));
			await wrapper.evaluate((element) => (element as HTMLElement).focus({ preventScroll: true }));
			await expect(wrapper).toBeFocused();
			expect.soft(await css(wrapper, 'outline-style')).toBe('solid');
			expect.soft(parseFloat(await css(wrapper, 'outline-width'))).toBeGreaterThanOrEqual(2);
			expect.soft(await css(wrapper, 'outline-color')).toBe(await tokenColor(page, '--color-indigo-400'));
			expect.soft(parseFloat(await css(wrapper, 'outline-offset'))).toBeLessThanOrEqual(0);
			await shot(page, 'agents-flat-' + width + '-' + scale + '-' + name + '-left');
			await filter.evaluate((element) => (element as HTMLElement).focus({ preventScroll: true }));
			await wrapper.evaluate((element, previous) => {
				if (previous === null) element.removeAttribute('tabindex'); else element.setAttribute('tabindex', previous);
			}, previousTabindex);
			await wrapper.evaluate((element) => { element.scrollLeft = element.scrollWidth; });
			await shot(page, 'agents-flat-' + width + '-' + scale + '-' + name + '-right');
			expect(await page.evaluate(() => document.documentElement.scrollWidth - innerWidth)).toBe(0);
		}
		await filters.getByRole('button', { name: /^All / }).press('Enter');
		const noor = table.locator('[data-agent=noor]');
		const link = noor.getByRole('link', { name: 'Noor Haddad', exact: true });
		await expect(link).toHaveAttribute('href', '/org-chart?member=noor');
		await link.focus(); await link.press('Enter');
		await expect(page).toHaveURL(/\/org-chart\?member=noor$/);
		await expect(page.getByTestId('selected-member')).toContainText('Noor Haddad');
		await page.goBack();
		await expect(table.getByTestId('agent-row')).toHaveCount(7);
		await visit(page, '/agents?org=empty');
		await page.locator('html').evaluate((html, pixels) => { html.style.fontSize = pixels + 'px'; }, 16 * scale);
		await expect(page.getByTestId('no-agents')).toContainText('No agents yet');
		await expect(table).toHaveCount(0);
		await shot(page, 'agents-flat-' + width + '-' + scale + '-empty', { context: page.getByTestId('no-agents') });
	});
}
