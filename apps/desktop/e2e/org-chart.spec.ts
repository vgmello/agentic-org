import { expect, test, type Page } from '@playwright/test';
import { visit } from './ready';
import { shot } from './shot';

async function openChart(page: Page, query = '') {
	await visit(page, '/create');
	await page.getByLabel('Organization name').fill('Acme');
	await page.getByRole('button', { name: 'Create organization' }).click();
	await expect(page).toHaveURL(/\/agents$/);
	await visit(page, `/org-chart${query}`);
	await expect(page.getByRole('tablist', { name: 'Org chart views' })).toBeVisible();
}

const card = (page: Page, id: string) => page.locator(`[data-testid=team-card][data-team=${id}]`);

test('OWNER-1: the owner is shown once, above the chart, and never on a team or in a count', async ({ page }) => {
	await openChart(page);
	const bar = page.getByRole('region', { name: 'Owner' });
	await expect(page.getByRole('region', { name: 'Owner' })).toHaveCount(1);
	await expect(bar).toContainText('Alex Kim (you) · Owner of Acme');
	await expect(bar.getByTestId('owner-sample')).toContainText('Sample data');
	await expect(bar).toContainText('Full access to every team, member and project');
	await expect(bar).toContainText('Any agent can message, ask or assign you');
	// Never as a tile on a card, in "Not in a team", or in a team's members.
	await expect(page.getByRole('button', { name: /^Alex Kim,/ })).toHaveCount(0);
	// Counts leave the owner out.
	await expect(page.getByText(/\b8\b.*agents/)).toBeVisible();
	await expect(page.getByText(/\b3\b.*people/)).toBeVisible();
});

test('TEAM-1: the Teams view draws the ownership tree with lead, agents, people and projects', async ({ page }) => {
	await openChart(page);
	await expect(page.getByTestId('team-card')).toHaveCount(5);
	await expect(card(page, 'eng')).toContainText('Lead: Sam Okafor');
	await expect(card(page, 'eng')).toContainText('2 teams');
	await expect(card(page, 'web')).toContainText('Agents 2');
	await expect(card(page, 'people')).toContainText('People 2');
	await expect(card(page, 'platform')).toContainText('Atlas');
	await expect(card(page, 'design')).not.toContainText('No lead');
	await shot(page, 'org-teams');
	// Sub-teams hang below their owner: Web is rendered inside Engineering's list item.
	await expect(page.locator('li', { has: card(page, 'eng') }).locator('[data-team=web]')).toHaveCount(1);
});

test('TEAM-1: collapsing a team hides the teams under it, and Expand all brings them back', async ({ page }) => {
	await openChart(page);
	await page.getByRole('button', { name: /collapse the teams under Engineering/ }).click();
	await expect(page.getByTestId('team-card')).toHaveCount(3);
	await shot(page, 'org-teams-collapsed');
	await page.getByRole('button', { name: 'Expand all' }).click();
	await expect(page.getByTestId('team-card')).toHaveCount(5);
	await page.getByRole('button', { name: 'Collapse all' }).click();
	await expect(page.getByTestId('team-card')).toHaveCount(3);
});

test('TEAM-2: a member in several teams appears in each, with a "+N teams" badge', async ({ page }) => {
	await openChart(page);
	const inWeb = card(page, 'web').getByRole('button', { name: /^Mira Castillo, agent, also in Platform/ });
	const inPlatform = card(page, 'platform').getByRole('button', { name: /^Mira Castillo, agent, also in Web/ });
	await expect(inWeb).toBeVisible();
	await expect(inPlatform).toBeVisible();
	await expect(inWeb.locator('xpath=..')).toContainText('+1');
});

test('TEAM-3: agents and people in no team are listed under "Not in a team"', async ({ page }) => {
	await openChart(page);
	const section = page.getByTestId('not-in-a-team');
	await expect(section.getByRole('button', { name: 'Avery Stone, person' })).toBeVisible();
	await expect(section.getByRole('button', { name: 'Quinn Adeyemi, agent' })).toBeVisible();
	await expect(section.getByRole('button')).toHaveCount(2);
});

test('TEAM-4 and PEOPLE-6: the office filter highlights placed agents, dims the rest, hides no one', async ({ page }) => {
	await openChart(page);
	const tiles = page.locator('[data-testid=team-card] button[aria-pressed], [data-testid=not-in-a-team] button[aria-pressed]');
	const before = await tiles.count();
	const filter = page.getByRole('combobox', { name: 'Office filter' });

	await filter.selectOption('unplaced');
	await expect(tiles).toHaveCount(before); // nobody hidden
	const ines = page.getByRole('button', { name: /^Inês Duarte, agent.*highlighted: Unplaced/ });
	await expect(ines).toContainText('Unplaced'); // the tag names the filter; screen readers hear it too
	await expect(page.locator('button[aria-label*="highlighted"]')).toHaveCount(1);
	await expect(page.getByRole('button', { name: /^Hana Weiss, person/ })).toHaveClass(/opacity-40/); // people are dimmed
	await expect(page.getByTestId('filter-status')).toContainText('Highlighting 1 agent not placed in an office');
	await expect(page.getByTestId('filter-announcement')).toHaveText('Highlighting 1 agent not placed in an office'); // announced
	await shot(page, 'org-teams-filter');

	// A real office: every placed agent (7 of the 8) is highlighted, and people never are (PEOPLE-6).
	const hq = (await filter.locator('option').allTextContents()).find((t) => t.startsWith('HQ'))!;
	await filter.selectOption({ label: hq });
	await expect(page.getByTestId('filter-status')).toContainText('Highlighting 7 agents at HQ');
	await expect(page.locator('button[aria-label*="highlighted: HQ"]').first()).toBeVisible();
	await expect(page.locator('[data-testid=team-card] button[aria-label*="highlighted"]').filter({ hasText: '' })).not.toHaveCount(0);
	await expect(page.getByRole('button', { name: /^Hana Weiss, person.*highlighted/ })).toHaveCount(0);

	await page.getByRole('button', { name: 'Clear' }).click();
	await expect(filter).toHaveValue('all');
	await expect(page.locator('button[aria-label*="highlighted"]')).toHaveCount(0);
	await expect(page.getByTestId('filter-status')).toHaveCount(0);
});

test('TEAM-12: agents and people are told apart without color, and screen readers hear which', async ({ page }) => {
	await openChart(page);
	const agent = card(page, 'platform').getByRole('button', { name: /^Kai Lindqvist, agent/ });
	const person = card(page, 'people').getByRole('button', { name: /^Hana Weiss, person/ });
	await expect(agent).toHaveClass(/rounded-md/); // a square tile
	await expect(agent.locator('svg')).toHaveCount(1); // with the robot glyph
	await expect(person).toHaveClass(/rounded-full/); // a round avatar
	await expect(person).toContainText('HW'); // with initials
});

test('TEAM-13: a project several teams work on shows "Shared" on each, listing the others', async ({ page }) => {
	await openChart(page);
	const shared = card(page, 'web').getByRole('button', { name: /^Shared project Beacon, also worked on by Design/ });
	await expect(shared).toBeVisible();
	await expect(card(page, 'design').getByRole('button', { name: /^Shared project Beacon, also worked on by Web/ })).toBeVisible();
	await expect(card(page, 'platform').getByRole('button', { name: /^Shared/ })).toHaveCount(0);
	await shared.click();
	await expect(card(page, 'web')).toContainText('Also: Design');
});

test('TEAM-14: projects with no team are noted above the chart and listed as unassigned', async ({ page }) => {
	await openChart(page);
	await expect(page.getByTestId('unassigned-note')).toContainText('1 project has no team yet');
	await expect(page.getByTestId('unassigned-projects')).toContainText('Scratchpad');
});

test('TEAM-15: a project shows its lead per team, or "No lead"', async ({ page }) => {
	await openChart(page);
	await expect(card(page, 'platform')).toContainText('Atlas · lead Kai Lindqvist');
});

test('TEAM-10: the same tree is available as a list, and the toggle is a real button', async ({ page }) => {
	await openChart(page);
	await page.getByRole('button', { name: 'List' }).click();
	const items = page.getByTestId('team-list-item');
	await expect(items).toHaveCount(5);
	await expect(items.first()).toContainText('Design');
	await expect(page.getByText('Sam Okafor (agent)').first()).toBeVisible();
	await expect(page.getByTestId('team-card')).toHaveCount(0);
	await page.getByRole('button', { name: 'Chart' }).click();
	await expect(page.getByTestId('team-card')).toHaveCount(5);
});

test('TEAM-11: three tabs; arrow keys move between them', async ({ page }) => {
	await openChart(page);
	const tabs = page.getByRole('tab');
	await expect(tabs).toHaveText(['Teams', 'People', 'Reporting lines']);
	await expect(tabs.first()).toHaveAttribute('aria-selected', 'true');
	await tabs.first().focus();
	await page.keyboard.press('ArrowRight');
	await expect(tabs.nth(1)).toHaveAttribute('aria-selected', 'true');
	await page.keyboard.press('ArrowLeft');
	await expect(tabs.first()).toHaveAttribute('aria-selected', 'true');
});

test('an organization with only its owner shows an empty state, not a blank chart', async ({ page }) => {
	await openChart(page, '?org=empty');
	await expect(page.getByText('No teams yet')).toBeVisible();
	await expect(page.getByTestId('unassigned-note')).toHaveCount(0);
	await expect(page.getByTestId('not-in-a-team')).toHaveCount(0);
	await shot(page, 'org-empty');
});

test('TEAM-1: "Fit to width" shrinks the chart, but never below 75% so tiles stay hittable; "Actual size" is full size', async ({ page }) => {
	await page.setViewportSize({ width: 1100, height: 800 });
	await openChart(page);
	const chart = page.getByTestId('teams-chart');
	await expect.poll(async () => (await chart.boundingBox())!.width).toBeGreaterThan(0);
	const fitted = (await chart.boundingBox())!.width;
	await page.getByRole('button', { name: 'Actual size' }).click();
	await expect(page.getByRole('button', { name: 'Fit to width' })).toBeVisible();
	await expect.poll(async () => (await chart.boundingBox())!.width).toBeGreaterThan(fitted);
	const actual = (await chart.boundingBox())!.width;
	expect(fitted / actual).toBeGreaterThanOrEqual(0.74); // the 75% floor
	expect(fitted).toBeLessThan(actual);
});

test('TEAM-13: the shared-project note opens and closes with the keyboard', async ({ page }) => {
	await openChart(page);
	const shared = card(page, 'web').getByRole('button', { name: /^Shared project Beacon/ });
	await shared.focus();
	await page.keyboard.press('Enter');
	await expect(card(page, 'web')).toContainText('Also: Design');
	await page.keyboard.press('Escape');
	await expect(card(page, 'web')).not.toContainText('Also: Design');
});

test('TEAM-11: Home and End jump to the first and last tab, and only the selected tab names its panel', async ({ page }) => {
	await openChart(page);
	const tabs = page.getByRole('tab');
	await tabs.first().focus();
	await page.keyboard.press('End');
	await expect(tabs.last()).toHaveAttribute('aria-selected', 'true');
	await expect(tabs.last()).toHaveAttribute('aria-controls', 'panel-reporting');
	await expect(tabs.first()).not.toHaveAttribute('aria-controls');
	await page.keyboard.press('Home');
	await expect(tabs.first()).toHaveAttribute('aria-selected', 'true');
});

test('the page says it shows sample data until the real organization is wired in', async ({ page }) => {
	await openChart(page);
	await expect(page.getByTestId('sample-data-note')).toContainText('Sample data');
});

test('TEAM-2: the "+N teams" badge says so, and selecting the member lists the other teams in words', async ({ page }) => {
	await openChart(page);
	const mira = card(page, 'web').getByRole('button', { name: /^Mira Castillo, agent/ });
	await expect(mira.locator('xpath=..')).toContainText('+1 team'); // beside the tile, not over it
	await mira.click();
	await expect(page.getByTestId('selected-member')).toContainText('Mira Castillo');
	await expect(page.getByTestId('selected-member')).toContainText('In Platform, Web');
	await page.getByRole('button', { name: 'Clear selection' }).click();
	await expect(page.getByTestId('selected-member')).toHaveCount(0);
});

test('TEAM-4 and TEAM-10: the office filter marks members in the list view too', async ({ page }) => {
	await openChart(page);
	await page.getByRole('combobox', { name: 'Office filter' }).selectOption('unplaced');
	await page.getByRole('button', { name: 'List' }).click();
	await expect(page.getByTestId('team-list-item').filter({ hasText: 'Inês Duarte (agent) — at Unplaced' })).toHaveCount(1);
	await expect(page.getByText('— at Unplaced')).toHaveCount(1);
	await expect(page.getByTestId('filter-announcement')).toContainText('Highlighting 1 agent');
});

test('the sample data is labelled where it can’t scroll away: next to the tabs and in the owner bar', async ({ page }) => {
	await openChart(page);
	await expect(page.getByTestId('sample-data-note')).toBeVisible();
	await expect(page.getByRole('region', { name: 'Owner' })).toContainText('Sample data');
});
