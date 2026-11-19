import { expect, test, type Page } from '@playwright/test';
import { visit } from './ready';
import { signInAsAdmin } from './web-session';

// Organization settings › Costs against the mock costs API (costs-and-insights-ux §1 Organization › Cost, §3, §4, §5;
// app-shell-ux §0.1). Mock: e2e/fakes/modules/costs.ts; its `?mock=` switches are listed there. The mock's clock is
// fixed at 19 Nov 2026 15:00 UTC and its records avoid period edges, so the figures hold in any time zone.
async function openOrganization(page: Page) {
	await signInAsAdmin(page, [
		{
			id: 'org-costs',
			name: 'Lumen Labs',
			created_at: '2026-11-18T00:00:00Z',
			recovery: 'acknowledged'
		}
	]);
	await visit(page, '/create');
	await page.getByLabel('Organization name').fill('Lumen Labs');
	await page.getByRole('button', { name: 'Create organization' }).click();
	// The first test of a run may wait on the dev server compiling the shell.
	await expect(page).not.toHaveURL(/\/create$/, { timeout: 30_000 });
}

const cost = (page: Page) => page.getByTestId('organization-cost');
const budgets = (page: Page) => page.getByTestId('agent-budgets');
const prices = (page: Page) => page.getByTestId('prices');
const tile = (page: Page, label: string) =>
	cost(page)
		.getByTestId('cost-totals')
		.locator('div')
		.filter({ has: page.locator('dt', { hasText: label }) });
const row = (page: Page, id: string) => cost(page).locator(`[data-row="${id}"]:visible`);

async function choosePeriod(page: Page, name: string) {
	await expect(page.getByRole('menu')).toHaveCount(0);
	await page.getByTestId('period-menu').click();
	await page.getByRole('menuitemradio', { name }).click();
	await expect(page.getByRole('menu')).toHaveCount(0);
}

async function noSidewaysScroll(page: Page) {
	const overflow = await page.evaluate(
		() => document.documentElement.scrollWidth - document.documentElement.clientWidth
	);
	expect(overflow).toBeLessThanOrEqual(0);
}

test('COST: the owner reads this month’s cost, its breakdowns and the records behind a figure', async ({
	page
}) => {
	await openOrganization(page);
	await visit(page, '/settings/costs');
	await expect(page).toHaveTitle('Costs · Organization settings · Agentic Org');
	await expect(page.getByRole('heading', { level: 2, name: 'Costs' })).toBeVisible();
	await expect(page.getByRole('heading', { level: 3, name: 'Organization cost' })).toBeVisible();

	// This month is the initial period; the totals say what they include and what is missing.
	await expect(page.getByTestId('period-menu')).toHaveText(/This month/);
	await expect(tile(page, 'Cost')).toContainText('≈ $123.90');
	await expect(tile(page, 'Cost')).toContainText(/(more|less) than|About the same/);
	await expect(tile(page, 'Usage records')).toContainText('48');
	await expect(cost(page)).toContainText('Includes API-equivalent cost for subscription plans');
	await expect(page.getByTestId('totals-incomplete')).toHaveText(
		'Usage incomplete: excludes 3 unpriced records · 2 records with a partial cost · 1 record without full token counts · 1 usage source not fully reported'
	);
	await expect(cost(page)).toContainText('Studio Mac office: not reported since 19 Nov, 13:40');

	// By project, largest first, closing with Not on a task with its overhead kept apart.
	await expect(row(page, 'project:atlas')).toContainText('≈ $76.84');
	await expect(row(page, 'project:beacon')).toContainText('≈ $24.90');
	await expect(row(page, 'project:not-on-a-task')).toContainText('Not on a task');
	await expect(row(page, 'project:not-on-a-task')).toContainText('Team overhead: none');
	await expect(row(page, 'project:not-on-a-task')).toContainText('organization overhead');
	await expect(row(page, 'project:beacon')).toContainText('Usage incomplete');

	// In-place tabs switch the breakdown; arrow keys move between them.
	const tabs = cost(page).getByRole('tab');
	await tabs.filter({ hasText: 'Project' }).focus();
	await page.keyboard.press('ArrowRight');
	await expect(tabs.filter({ hasText: 'Consumer' })).toBeFocused();
	await expect(tabs.filter({ hasText: 'Consumer' })).toHaveAttribute('aria-selected', 'true');
	await expect(row(page, 'consumer:sam')).toContainText('Sam Okafor');
	await expect(row(page, 'consumer:sam')).toContainText('≈ $51.02');
	await expect(row(page, 'consumer:one-off')).toContainText('One-off model calls');
	// An agent whose runtime reports no usage has no known cost: a dash, never zero.
	await expect(row(page, 'consumer:juno')).toContainText('—');
	await expect(row(page, 'consumer:juno')).not.toContainText('$0.00');
	await tabs.filter({ hasText: 'Work type' }).click();
	await expect(row(page, 'work:build')).toContainText('≈ $68.99');
	await expect(row(page, 'work:not_recorded')).toContainText('Not recorded');

	// A figure opens the records behind it; Load more keeps the place; closing returns to the figure.
	await tabs.filter({ hasText: 'Project' }).click();
	const figure = row(page, 'project:atlas').getByRole('button', {
		name: /Open the records for Atlas/
	});
	await figure.click();
	const sheet = page.getByRole('dialog');
	await expect(sheet.getByRole('heading', { name: 'Usage records: Atlas' })).toBeVisible();
	await expect(sheet).toContainText('22 records');
	const records = sheet.locator('[data-record]');
	await expect(records).toHaveCount(20);
	await expect(records.first()).toContainText(/Runtime estimate|Token estimate/);
	await expect(records.first()).toContainText('Input');
	await expect(records.first()).toContainText('cache write');
	await sheet.getByRole('button', { name: 'Load more' }).click();
	await expect(records).toHaveCount(22);
	await expect(records.nth(20)).toBeFocused();
	await expect(sheet).toContainText('Showing 22 of 22');
	// The corrected record says who moved it and why.
	await expect(sheet.getByTestId('record-correction')).toContainText('Alex Kim');
	await page.keyboard.press('Escape');
	await expect(sheet).toBeHidden();
	await expect(figure).toBeFocused();
});

test('COST: the period menu changes the figures; a custom range is checked at its field', async ({
	page
}) => {
	await openOrganization(page);
	await visit(page, '/settings/costs');
	await expect(tile(page, 'Cost')).toContainText('≈ $123.90');

	await choosePeriod(page, 'Last month');
	await expect(page.getByTestId('period-menu')).toHaveText(/Last month/);
	await expect(tile(page, 'Cost')).toContainText('≈ $278.66');
	await expect(page.getByTestId('period-range')).toContainText('1 Oct – 31 Oct 2026');
	// A period that ended has no unreported sources to wait on.
	await expect(cost(page)).not.toContainText('Studio Mac office');

	await choosePeriod(page, 'This week');
	await expect(tile(page, 'Cost')).toContainText('≈ $35.81');

	await choosePeriod(page, 'Custom');
	const from = page.getByLabel('From');
	await expect(from).toBeFocused();
	await from.fill('2026-11-10');
	await page.getByLabel('To (optional)').fill('2026-11-02');
	await page.getByRole('button', { name: 'Show' }).click();
	await expect(page.getByLabel('To (optional)')).toBeFocused();
	await expect(page.getByLabel('To (optional)')).toHaveAttribute('aria-invalid', 'true');
	await expect(cost(page)).toContainText("The last day can't be before the first day.");
	// The figures still describe the last complete choice.
	await expect(tile(page, 'Cost')).toContainText('≈ $35.81');
	await page.getByLabel('From').fill('2026-10-01');
	await page.getByLabel('To (optional)').fill('2026-10-31');
	await page.getByRole('button', { name: 'Show' }).click();
	await expect(tile(page, 'Cost')).toContainText('≈ $278.66');
	await expect(page.getByTestId('period-menu')).toHaveText(/Custom/);

	await choosePeriod(page, 'All time');
	await expect(tile(page, 'Cost')).toContainText('≈ $517.28');
	await expect(tile(page, 'Cost')).not.toContainText(/more than|less than/);
});

test('BUD: agent budgets show in the budget zone, with stopping and paused states in words', async ({
	page
}) => {
	await openOrganization(page);
	await visit(page, '/settings/costs');
	await expect(
		budgets(page).getByRole('heading', { level: 3, name: 'Agent budgets' })
	).toBeVisible();
	await expect(budgets(page)).toContainText("America/New_York, the owner's time zone");
	const agent = (id: string) => budgets(page).locator(`[data-agent="${id}"]`);
	await expect(agent('sam').locator('[data-budget="daily"]')).toContainText(
		'Today (America/New_York)'
	);
	await expect(agent('sam').locator('[data-budget="daily"]')).toContainText(
		'≈ $7.87 of $9.00 · 87%'
	);
	await expect(agent('sam').locator('[data-budget="daily"]')).toContainText('80% reached');
	await expect(agent('sam').locator('[data-budget="monthly"]')).toContainText(
		'This month (America/New_York)'
	);
	await expect(agent('mira').getByTestId('budget-state')).toHaveText('Stopping · Budget reached');
	await expect(agent('kai').getByTestId('budget-state')).toHaveText('Paused · Budget reached');
	await expect(agent('kai')).toContainText('This budget already used its one 20% extension.');
	await expect(agent('noor')).toContainText('18.1 M of 25 M tokens · 72%');
	await expect(agent('juno')).toContainText(
		'This runtime does not report usage, so this budget cannot stop its unmeasured work.'
	);
	await expect(agent('kai').getByRole('meter')).toHaveAttribute('aria-valuenow', '100');
	await expect(budgets(page).getByTestId('no-budget')).toContainText('Pax Moreau');
	await expect(agent('sam').getByRole('link', { name: 'Sam Okafor' })).toHaveAttribute(
		'href',
		'/members/sam/cost'
	);

	await visit(page, '/settings/costs?mock=no-zone');
	await expect(budgets(page).getByTestId('budget-zone-unknown')).toContainText(
		"Agent budgets can't run yet: the owner's time zone isn't known"
	);
});

test('PRICE: the owner reads versioned rates and adds one; refusals focus their field', async ({
	page
}) => {
	await openOrganization(page);
	await visit(page, '/settings/costs');
	const opus = prices(page).locator('[data-price-model="claude-opus-4-1"]');
	await expect(opus).toContainText('In effect');
	await expect(opus).toContainText('$5.00');
	await expect(opus).toContainText('in use, never changes');
	await opus.getByText('Earlier rates (1)').click();
	await expect(opus).toContainText('$15.00');
	const codex = prices(page).locator('[data-price-model="gpt-5-codex"]');
	await expect(codex.getByTestId('scheduled-rate')).toContainText('Scheduled');
	// An unknown rate is a dash with its reason, not zero.
	await expect(
		codex.getByText('No rate: records with these tokens are unpriced').first()
	).toBeAttached();

	const add = prices(page).getByRole('button', { name: 'Add a rate…' });
	await add.click();
	const dialog = page.getByRole('dialog');
	await expect(dialog.getByRole('heading', { name: 'Add a rate' })).toBeVisible();
	await dialog.getByRole('button', { name: 'Add rate' }).click();
	await expect(page.getByLabel('Provider')).toBeFocused();
	await expect(page.getByLabel('Provider')).toHaveAttribute('aria-invalid', 'true');
	await expect(dialog).toContainText('Name the provider, such as anthropic.');

	await page.getByLabel('Provider').fill('openai');
	await page.getByLabel('Model').fill('gpt-5-mini');
	await page.getByLabel('Input').fill('0.25');
	await page.getByLabel('Output').fill('2');
	await dialog.getByRole('button', { name: 'Add rate' }).click();
	await expect(dialog).toBeHidden();
	await expect(add).toBeFocused();
	await expect(page.getByText(/Rate added: gpt-5-mini from/)).toBeVisible();
	const mini = prices(page).locator('[data-price-model="gpt-5-mini"]');
	await expect(mini).toContainText('$0.25');
	await expect(mini).toContainText('$2.00');

	// A second rate from the same day is refused at the date, with the values kept.
	const again = mini.getByRole('button', { name: 'New rate for gpt-5-mini' });
	await again.click();
	await expect(dialog.getByRole('heading', { name: 'New rate for gpt-5-mini' })).toBeVisible();
	await expect(page.getByLabel('Input')).toHaveValue('0.25');
	await page.getByLabel('Output').fill('1.5');
	await dialog.getByRole('button', { name: 'Add rate' }).click();
	await expect(page.getByLabel('Takes effect on')).toBeFocused();
	await expect(dialog).toContainText(
		'gpt-5-mini already has a rate from that day. Choose another day.'
	);
	await expect(page.getByLabel('Output')).toHaveValue('1.5');
	await page.keyboard.press('Escape');
	await expect(again).toBeFocused();
});

test('PRICE: an unknown outcome is checked before anything is sent again; a verified failure keeps the values', async ({
	page
}) => {
	await openOrganization(page);
	await visit(page, '/settings/costs?mock=save-unknown');
	await prices(page).getByRole('button', { name: 'Add a rate…' }).click();
	const dialog = page.getByRole('dialog');
	await page.getByLabel('Provider').fill('openai');
	await page.getByLabel('Model').fill('gpt-5-nano');
	await page.getByLabel('Input').fill('0.05');
	await dialog.getByRole('button', { name: 'Add rate' }).click();
	await expect(dialog.getByTestId('add-rate-outcome')).toContainText('Save not confirmed');
	await expect(dialog.getByRole('button', { name: 'Add rate' })).toHaveCount(0);
	await dialog.getByRole('button', { name: 'Check status' }).click();
	await expect(dialog).toBeHidden();
	await expect(prices(page).locator('[data-price-model="gpt-5-nano"]')).toHaveCount(1);

	await visit(page, '/settings/costs?mock=save-fails');
	await prices(page).getByRole('button', { name: 'Add a rate…' }).click();
	await page.getByLabel('Provider').fill('openai');
	await page.getByLabel('Model').fill('gpt-5-nano');
	await page.getByLabel('Input').fill('0.05');
	await dialog.getByRole('button', { name: 'Add rate' }).click();
	await expect(dialog.getByTestId('add-rate-outcome')).toContainText("Couldn't add the rate");
	await expect(page.getByLabel('Model')).toHaveValue('gpt-5-nano');
	await expect(dialog.getByRole('button', { name: 'Add rate' })).toBeVisible();
});

test('COST-9 scope: a viewer who is not the owner sees no amounts and cannot add rates', async ({
	page
}) => {
	await openOrganization(page);
	await visit(page, '/settings/costs?mock=viewer');
	await expect(cost(page).getByTestId('cost-not-yours')).toContainText(
		"Only the organization's owner sees its cost."
	);
	await expect(cost(page)).not.toContainText('$');
	await expect(page.getByTestId('agent-budgets')).toHaveCount(0);
	await expect(page.getByTestId('period-menu')).toHaveCount(0);
	await expect(prices(page).getByTestId('prices-read-only')).toHaveText(
		'Only the owner adds rates.'
	);
	await expect(prices(page).getByRole('button')).toHaveCount(0);
	await expect(prices(page).locator('[data-price-model="claude-opus-4-1"]')).toContainText('$5.00');
});

test('States: loading, error with Retry, empty, unavailable and a failing records sheet', async ({
	page
}) => {
	await openOrganization(page);
	await visit(page, '/settings/costs?mock=slow');
	await expect(page.getByText("Loading the organization's cost")).toBeAttached();
	await expect(page.getByText('Loading prices')).toBeAttached();
	await expect(tile(page, 'Cost')).toContainText('≈ $123.90');

	// HQ unreachable for every part: one failure with one Try again (§1 States), not three.
	await visit(page, '/settings/costs?mock=fails');
	await expect(
		page.getByRole('heading', { name: "Couldn't load costs. Try again." })
	).toBeVisible();
	await expect(page.getByRole('button', { name: 'Try again' })).toHaveCount(1);
	await expect(page.getByTestId('organization-cost')).toHaveCount(0);

	await visit(page, '/settings/costs?mock=empty');
	await expect(cost(page).getByRole('heading', { name: 'No spend in this period' })).toBeVisible();
	await expect(prices(page)).toContainText('No prices set: unpriced records add no known dollars.');
	await expect(prices(page).getByRole('button', { name: 'Add a rate…' })).toBeVisible();

	await visit(page, '/settings/costs?mock=unavailable');
	await expect(page.getByRole('heading', { name: "Costs isn't built yet" })).toBeVisible();
	await expect(page.getByTestId('organization-cost')).toHaveCount(0);

	await visit(page, '/settings/costs?mock=records-fail');
	await row(page, 'project:atlas')
		.getByRole('button', { name: /Open the records/ })
		.click();
	await expect(
		page.getByRole('dialog').getByRole('heading', { name: "Couldn't load the usage records" })
	).toBeVisible();
});

test('Arriving from the settings navigation focuses the page heading', async ({ page }) => {
	await openOrganization(page);
	await visit(page, '/settings');
	await page
		.getByRole('navigation', { name: 'Organization settings' })
		.getByRole('link', { name: 'Costs' })
		.first()
		.click();
	await expect(page).toHaveURL(/\/settings\/costs$/);
	await expect(page.getByRole('heading', { level: 2, name: 'Costs' })).toBeFocused();
});

test('320 px and 200% text: no sideways scroll, 44 px targets, whole-word pills', async ({
	page
}) => {
	await openOrganization(page);
	await page.setViewportSize({ width: 320, height: 800 });
	await visit(page, '/settings/costs');
	await expect(tile(page, 'Cost')).toContainText('≈ $123.90');
	await noSidewaysScroll(page);
	for (const control of [
		page.getByTestId('period-menu'),
		row(page, 'project:atlas').getByRole('button', { name: /Open the records/ }),
		prices(page).getByRole('button', { name: 'Add a rate…' }),
		cost(page).getByRole('tab', { name: 'Consumer' })
	]) {
		const box = await control.boundingBox();
		expect(box?.height ?? 0).toBeGreaterThanOrEqual(44);
	}
	await page.evaluate(() => (document.documentElement.style.fontSize = '200%'));
	await noSidewaysScroll(page);
	const pill = budgets(page).locator('[data-agent="mira"]').getByTestId('budget-state');
	await expect(pill).toHaveText('Stopping · Budget reached');
	const fits = await pill.evaluate((el) => el.scrollWidth <= el.clientWidth + 1);
	expect(fits).toBe(true);
});
