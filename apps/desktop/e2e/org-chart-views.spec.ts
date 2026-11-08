import { expect, test, type Page } from '@playwright/test';
import { visit } from './ready';
import { shot } from './shot';

async function openChart(page: Page) {
	await visit(page, '/create');
	await page.getByLabel('Organization name').fill('Acme');
	await page.getByRole('button', { name: 'Create organization' }).click();
	await expect(page).toHaveURL(/\/agents$/);
	await visit(page, '/org-chart');
	await expect(page.getByRole('tablist', { name: 'Org chart views' })).toBeVisible();
}
const tab = (page: Page, name: string) => page.getByRole('tab', { name });
const people = (page: Page, id: string) => page.locator(`[data-testid=people-card][data-team=${id}]`);
const node = (page: Page, id: string) => page.locator(`[data-testid=report-node][data-member=${id}]`);

test.describe('People view', () => {
	test.beforeEach(async ({ page }) => {
		await openChart(page);
		await tab(page, 'People').click();
	});

	test('PEOPLE-1: the same team tree with only people on the cards; an agent only names a lead', async ({ page }) => {
		await expect(page.getByTestId('people-card')).toHaveCount(5);
		await expect(people(page, 'people')).toContainText('Hana Weiss');
		await expect(people(page, 'people')).toContainText('Leo Brandt');
		await expect(people(page, 'people')).not.toContainText('Inês Duarte'); // an agent that isn't the lead
		await expect(people(page, 'people')).toContainText('Lead: Hana Weiss'); // a person who leads is named too
		await expect(people(page, 'people')).toContainText('lead'); // and tagged on their row
		await expect(people(page, 'eng')).toContainText('Lead: Sam Okafor (agent)'); // an agent shows to name the lead
		await expect(people(page, 'web')).toContainText('No people');
		await expect(page.getByTestId('people-chart').getByRole('button', { name: /, agent/ })).toHaveCount(0); // no agent tiles
		await shot(page, 'org-people');
	});

	test('PEOPLE-2: it always says agents can only reach the people listed', async ({ page }) => {
		await expect(page.getByTestId('people-note')).toContainText('Agents can only reach the people listed in your organization');
	});

	test('PEOPLE-6: people never show an office, and the office filter is not offered here', async ({ page }) => {
		await expect(page.getByRole('combobox', { name: 'Office filter' })).toHaveCount(0);
		await expect(page.getByTestId('people-chart')).not.toContainText('HQ');
		await expect(page.getByTestId('people-chart').locator('[aria-label*="highlighted"]')).toHaveCount(0);
	});

	test('people in no team are listed apart, and the collapse controls work', async ({ page }) => {
		await expect(page.getByTestId('people-not-in-a-team').getByRole('button', { name: 'Avery Stone, person' })).toBeVisible();
		await page.getByRole('button', { name: /collapse the teams under Engineering/ }).click();
		await expect(page.getByTestId('people-card')).toHaveCount(3);
		await page.getByRole('button', { name: 'Expand all' }).click();
		await expect(page.getByTestId('people-card')).toHaveCount(5);
	});
});

test.describe('Reporting lines view', () => {
	test.beforeEach(async ({ page }) => {
		await openChart(page);
		await tab(page, 'Reporting lines').click();
	});

	test('REPORT-1: the owner is the root, and everyone appears exactly once', async ({ page }) => {
		await expect(page.getByTestId('report-node').first()).toContainText('Alex Kim');
		await expect(page.getByTestId('report-node').first()).toContainText('Owner (you)');
		await expect(page.getByTestId('report-node')).toHaveCount(12); // 11 members + the owner
		const names = await page.getByTestId('report-node').locator('p.font-semibold').allTextContents();
		expect(new Set(names).size).toBe(names.length);
		// Quinn has no manager: directly under the owner.
		await expect(page.locator('li', { has: node(page, 'you') }).locator('> ul > li', { has: node(page, 'quinn') })).toHaveCount(1);
		await shot(page, 'org-reporting');
	});

	test('REPORT-2: each node shows its name, Agent or Person, its teams and its number of reports', async ({ page }) => {
		await expect(node(page, 'sam')).toContainText('Sam Okafor');
		await expect(node(page, 'sam')).toContainText('Agent');
		await expect(node(page, 'sam')).toContainText('Engineering');
		await expect(node(page, 'sam')).toContainText('3 reports');
		await expect(node(page, 'hana')).toContainText('Person');
		await expect(node(page, 'hana')).toContainText('2 reports');
		await expect(node(page, 'quinn')).toContainText('Not in a team');
		await expect(node(page, 'mira')).toContainText('Platform, Web');
		await expect(node(page, 'kai')).toContainText('0 reports');
	});

	test('REPORT-5: it says reporting lines are separate from teams', async ({ page }) => {
		await expect(page.getByTestId('reporting-note')).toContainText("They are separate from teams");
	});

	test('Collapse all folds every manager, Expand all unfolds them', async ({ page }) => {
		await page.getByRole('button', { name: 'Collapse all' }).click();
		await expect(page.getByTestId('report-node')).toHaveCount(3); // the owner and the owner's direct reports (Avery, Quinn)
		await page.getByRole('button', { name: 'Expand all' }).click();
		await expect(page.getByTestId('report-node')).toHaveCount(12);
	});

	test('collapsing a manager hides the people under them', async ({ page }) => {
		await page.getByRole('button', { name: /collapse the people reporting to Sam Okafor/ }).click();
		await expect(page.getByTestId('report-node')).toHaveCount(9);
		await page.getByRole('button', { name: 'Expand all' }).click();
		await expect(page.getByTestId('report-node')).toHaveCount(12);
	});
});

test('TEAM-11: switching tabs keeps the selected member', async ({ page }) => {
	await openChart(page);
	await page.getByTestId('team-card').first().getByRole('button', { name: /^Juno Park, agent/ }).click();
	await expect(page.getByRole('button', { name: /^Juno Park, agent/ }).first()).toHaveAttribute('aria-pressed', 'true');
	await tab(page, 'Reporting lines').click();
	await expect(node(page, 'juno').getByRole('button', { name: /^Juno Park, agent/ })).toHaveAttribute('aria-pressed', 'true');
	await tab(page, 'Teams').click();
	await expect(page.getByRole('button', { name: /^Juno Park, agent/ }).first()).toHaveAttribute('aria-pressed', 'true');
	// People view: pick a person; it stays selected back on Teams.
	await tab(page, 'People').click();
	await page.getByTestId('people-chart').getByRole('button', { name: /^Hana Weiss, person/ }).click();
	await tab(page, 'Teams').click();
	await expect(page.getByTestId('team-card').getByRole('button', { name: /^Hana Weiss, person/ })).toHaveAttribute('aria-pressed', 'true');
});
