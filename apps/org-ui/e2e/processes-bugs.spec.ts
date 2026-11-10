import { expect, test, type Page } from '@playwright/test';
import { visit } from './ready';
import { shot } from './shot';

test.use({ locale: 'en-GB', timezoneId: 'UTC' });

async function open(page: Page, query = '') {
	await visit(page, '/create');
	await page.getByLabel('Organization name').fill('Acme');
	await page.getByRole('button', { name: 'Create organization' }).click();
	await expect(page).toHaveURL(/\/agents$/);
	await visit(page, `/processes${query}`);
	await expect(page.getByTestId('process-list')).toBeVisible();
}
const row = (page: Page, key: string) => page.locator(`[data-testid=process-row][data-key="${key}"]`);

// Substitute only sample data in the served dev modules; the production API and UI get no test hook.
async function sampleNames(page: Page, names: Record<string, string>) {
	await page.route(/\/src\/lib\/modules\/processes\/(sample|world)\.ts(?:\?.*)?$/, async (route) => {
		const response = await route.fetch();
		let body = await response.text();
		for (const [original, replacement] of Object.entries(names)) {
			body = body.replaceAll(`'${original}'`, JSON.stringify(replacement)).replaceAll(`"${original}"`, JSON.stringify(replacement));
		}
		await route.fulfill({ response, body });
	});
}

test('PROC-9 / BUG-14: drift badges leave a readable name column at 960x640', async ({ page }) => {
	await page.setViewportSize({ width: 960, height: 640 });
	await open(page, '?place=team:web');
	const review = row(page, 'code-review');
	await expect(review).toContainText('Parent changed: Organization is now v2');
	await shot(page, 'processes-bug14-drift-960');
	const title = review.getByRole('button').locator('span.font-semibold');
	const bounds = await title.boundingBox();
	expect(bounds!.width).toBeGreaterThanOrEqual(192);
	expect(bounds!.height).toBeLessThanOrEqual(48);
	await review.getByRole('button').focus();
	await page.keyboard.press('Enter');
	await expect(page.getByRole('dialog').getByRole('heading', { name: 'Code review (Web)' })).toBeVisible();
});

test('PROC-16 / BUG-15: invalid details keep the colon space and sentence case', async ({ page }) => {
	await open(page, '?place=team:design');
	const problem = page.getByTestId('problem').filter({ hasText: 'Design handoff' });
	await shot(page, 'processes-bug15-invalid');
	await expect(problem).toHaveText('Design handoff: details not recognized (overridable).');
});

test('PROC-16 / BUG-15: guidance moments separate the label from its prompt', async ({ page }) => {
	await open(page);
	await row(page, 'branch-and-merge').click();
	const sheet = page.getByRole('dialog');
	await expect(sheet.getByRole('heading', { name: 'Prompts at guidance moments' })).toBeVisible();
	await shot(page, 'processes-bug15-moments', { context: sheet });
	await expect(sheet.locator('dl').filter({ hasText: 'Starting a task' })).toContainText('Starting a task: Check that your open branch is merged');
});

test('PROC-1 / BUG-15: a clash formats its key as code without literal backticks', async ({ page }) => {
	await open(page, '?place=project:bea');
	const clashes = page.getByTestId('problem').filter({ hasText: 'Two processes use the key' });
	await expect(clashes).toHaveCount(2);
	await shot(page, 'processes-bug15-clash');
	await expect(clashes.first().locator('code')).toHaveText('standup');
	await expect(clashes.first()).toContainText('Standup: Two processes use the key standup in Beacon.');
	await expect(clashes.first()).not.toContainText('`');
});

test('PROC-9 / BUG-16: added here names only the place that owns the addition', async ({ page }) => {
	await open(page, '?place=team:eng');
	await expect(row(page, 'incident-response').getByTestId('kind')).toHaveText('Addition (added here)');
	for (const place of ['team:web', 'team:platform', 'project:atl', 'agent:noor']) {
		await page.getByLabel('Show processes for').selectOption(place);
		await expect(page.getByLabel('Show processes for')).toHaveValue(place);
		const addition = row(page, 'incident-response');
		await expect(addition).toContainText('Team Engineering v1');
		await shot(page, `processes-bug16-inherited-${place.replace(':', '-')}`);
		await expect(addition.getByTestId('kind')).toHaveText('Addition');
	}
});

test('PROC-9 / BUG-16: an agent source uses the person name in its row and sheet chain', async ({ page }) => {
	await open(page, '?place=agent:noor');
	const notes = row(page, 'my-notes');
	await shot(page, 'processes-bug16-agent-name');
	await expect(notes.getByTestId('level-badge')).toHaveText('Agent Noor Haddad v1');
	await notes.click();
	const sheet = page.getByRole('dialog');
	await expect(sheet.getByTestId('level-badge').first()).toHaveText('Agent Noor Haddad v1');
	await expect(sheet.getByTestId('why-layer').getByTestId('level-badge')).toHaveText('Agent Noor Haddad v1');
	await shot(page, 'processes-bug16-agent-sheet', { context: sheet });
});

test('PROC-9 / BUG-17: long team, project and agent names stay within the minimum window', async ({ page }) => {
	await page.setViewportSize({ width: 960, height: 640 });
	const longNames = { Web: 'Web'.repeat(90), Atlas: 'Atlas'.repeat(90), 'Noor Haddad': 'Noor'.repeat(90) };
	await sampleNames(page, longNames);
	await open(page);
	for (const [place, name] of [['team:web', longNames.Web], ['project:atl', longNames.Atlas], ['agent:noor', longNames['Noor Haddad']]]) {
		await page.getByLabel('Show processes for').selectOption(place);
		await expect(page.getByRole('heading', { name: `Processes · ${name} (${place.split(':')[0]})` })).toBeVisible();
		await shot(page, `processes-bug17-long-${place.replace(':', '-')}-960`);
		const screen = await page.getByTestId('process-list').locator('..').evaluate((element) => ({
			width: element.clientWidth, scrollWidth: element.scrollWidth, right: element.getBoundingClientRect().right, viewport: innerWidth,
			overflow: [...element.querySelectorAll('*')].filter((child) => child.getBoundingClientRect().right > innerWidth + 1 && !['OPTION', 'OPTGROUP'].includes(child.tagName)).map((child) => ({ tag: child.tagName, text: child.textContent?.slice(0, 80), class: child.className })).slice(0, 10)
		}));
		expect(screen.scrollWidth, JSON.stringify(screen.overflow)).toBeLessThanOrEqual(screen.width + 1);
		expect(screen.right).toBeLessThanOrEqual(screen.viewport);
		const select = await page.getByLabel('Show processes for').boundingBox();
		expect(select!.x + select!.width).toBeLessThanOrEqual(960 - 32);
	}
});

test('PROC-9 / BUG-17: an unbroken process title wraps clear of the sheet close control', async ({ page }) => {
	await page.setViewportSize({ width: 960, height: 640 });
	const longTitle = 'Process'.repeat(50);
	await sampleNames(page, { 'Branch, review and merge': longTitle });
	await open(page);
	await expect(row(page, 'branch-and-merge')).toContainText(longTitle);
	await row(page, 'branch-and-merge').click();
	const sheet = page.getByRole('dialog');
	const title = sheet.getByRole('heading', { name: longTitle, exact: true });
	await expect(title).toBeVisible();
	await shot(page, 'processes-bug17-long-sheet-960', { context: sheet });
	const textRight = await title.evaluate((element) => {
		const range = document.createRange();
		range.selectNodeContents(element);
		return range.getBoundingClientRect().right;
	});
	const close = await sheet.getByRole('button', { name: 'Close', exact: true }).boundingBox();
	expect(textRight).toBeLessThanOrEqual(close!.x - 8);
});

test('PROC-9 / BUG-17: a turned-off sheet wraps a long source name and explanation', async ({ page }) => {
	await page.setViewportSize({ width: 960, height: 640 });
	const project = 'Atlas'.repeat(90);
	await sampleNames(page, { Atlas: project });
	await open(page, '?place=project:atl');
	await expect(page.getByRole('heading', { name: `Processes · ${project} (project)` })).toBeVisible();
	await page.getByTestId('turned-off-row').click();
	const sheet = page.getByRole('dialog');
	await expect(sheet.getByTestId('off-why')).toContainText(`Turned off for ${project}: we report daily.`);
	await shot(page, 'processes-bug17-long-turned-off-960', { context: sheet });
	const bounds = await sheet.evaluate((element) => ({ width: element.clientWidth, scrollWidth: element.scrollWidth }));
	expect(bounds.scrollWidth).toBeLessThanOrEqual(bounds.width + 1);
	await sheet.getByTestId('off-why').scrollIntoViewIfNeeded();
	await shot(page, 'processes-bug17-long-off-reason-960', { context: sheet });
});

test('PROC-9 / BUG-18: the process side panel is 440px at the minimum window', async ({ page }) => {
	await page.setViewportSize({ width: 960, height: 640 });
	await open(page);
	await row(page, 'branch-and-merge').click();
	const sheet = page.getByRole('dialog');
	await expect(sheet).toBeVisible();
	await shot(page, 'processes-bug18-panel-440', { context: sheet });
	expect((await sheet.boundingBox())!.width).toBe(440);
});

test('PROC-18 / BUG-18: keyboard Close has the token focus ring and restores focus', async ({ page }) => {
	await open(page);
	const trigger = row(page, 'branch-and-merge').getByRole('button');
	await trigger.focus();
	await page.keyboard.press('Enter');
	const sheet = page.getByRole('dialog');
	const close = sheet.getByRole('button', { name: 'Close', exact: true });
	await expect(close).toBeFocused();
	await shot(page, 'processes-bug18-close-focus', { context: sheet });
	const ring = await close.evaluate((element) => {
		const style = getComputedStyle(element);
		const token = document.createElement('span');
		token.style.color = 'var(--ring)';
		document.body.append(token);
		const color = getComputedStyle(token).color;
		token.remove();
		return { width: style.outlineWidth, offset: style.outlineOffset, color: style.outlineColor, token: color };
	});
	expect(ring.width).toBe('2px');
	expect(ring.offset).toBe('2px');
	expect(ring.color).toBe(ring.token);
	await page.keyboard.press('Escape');
	await expect(sheet).toBeHidden();
	await expect(trigger).toBeFocused();
});

for (const [now, dated] of [['2026-11-10T12:00:00Z', true], ['2026-11-08T12:00:00Z', false]] as const) {
	test(`PROC-16 / BUG-20: a ${dated ? 'previous-day' : 'same-day'} handbook copy shows ${dated ? 'its date and time' : 'its time'}`, async ({ page }) => {
		await page.clock.setFixedTime(new Date(now));
		await open(page, '?handbook=down');
		const warning = page.getByTestId('handbook-down');
		await shot(page, dated ? 'processes-bug20-dated-copy' : 'processes-bug20-today-copy');
		await expect(warning).toContainText('09:12');
		if (dated) await expect(warning).toContainText('8 Nov 2026');
		else await expect(warning).not.toContainText('2026');
	});
}
