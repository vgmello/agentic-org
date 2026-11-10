import { expect, test, type Page } from '@playwright/test';
import { visit } from './ready';
import { shot } from './shot';

async function openEmptyChart(page: Page) {
	await visit(page, '/create');
	await page.getByLabel('Organization name').fill('Agentic Consulting');
	await page.getByRole('button', { name: 'Create organization' }).click();
	await expect(page).toHaveURL(/\/agents$/);
	await visit(page, '/org-chart?org=empty');
	await expect(page.getByRole('tablist', { name: 'Org chart views' })).toBeVisible();
}
const dialog = (page: Page) => page.getByTestId('bring-in');
const row = (page: Page, key: string) => page.locator(`[data-testid=import-row][data-row="${key}"]`);

async function scanToReview(page: Page, hints = false) {
	await page.getByTestId('bring-in-empty').click();
	await expect(dialog(page).getByTestId('bring-in-sources')).toContainText('Found 5 agents in 1 project (ac)');
	await expect(dialog(page).getByTestId('bring-in-sources')).toContainText('Found 9 names, 5 not in the HR workspace');
	await expect(dialog(page).getByTestId('hints-source')).toContainText('A teams.toml that says who is in which team.');
	await shot(page, 'bring-in-sources');
	if (hints) await dialog(page).getByRole('button', { name: 'Choose a team hints file' }).click();
	await dialog(page).getByRole('button', { name: 'Scan' }).click();
	await expect(dialog(page).getByTestId('bring-in-review')).toBeVisible();
}

test('IMP-1..4: scanning lists every agent once, with its sources, and counts only the complete rows', async ({ page }) => {
	await openEmptyChart(page);
	await scanToReview(page);
	await expect(page.getByTestId('import-row')).toHaveCount(10);
	await expect(row(page, 'ac-staff-engineer-ada')).toContainText('HR + messaging');
	await expect(row(page, 'server-dev')).toContainText('Messaging only');
	await expect(row(page, 'ac-software-engineer-riley')).toContainText('HR only');
	await expect(row(page, 'ac-software-engineer-riley')).toContainText('No messages from Riley yet');
	await expect(dialog(page).getByRole('button', { name: /^Bring in 5 agents · 5 need details$/ })).toBeVisible();
	await expect(dialog(page).getByTestId('what-happens')).toContainText('Creates 5 agents, 2 teams and 1 project in Agentic Consulting. Nothing changes in your HR workspace or in messaging.');
	await shot(page, 'bring-in-review');
	// Filling a messaging-only row raises the count.
	await dialog(page).getByLabel('Role for server-dev').selectOption('software engineer');
	await expect(dialog(page).getByLabel('Title for server-dev')).toHaveValue('Software Engineer');
	await dialog(page).getByLabel('Runtime for server-dev').selectOption('codex');
	await expect(dialog(page).getByRole('button', { name: /^Bring in 6 agents · 4 need details$/ })).toBeVisible();
	// Skipping a row takes it out of both counts.
	await dialog(page).getByLabel('Bring in reviewer').selectOption('skip');
	await expect(dialog(page).getByRole('button', { name: /^Bring in 6 agents · 3 need details$/ })).toBeVisible();
});

test('IMP-10: the team hints file prefills the teams, the leads and the members', async ({ page }) => {
	await openEmptyChart(page);
	await scanToReview(page, true);
	await expect(dialog(page).getByTestId('teams-line')).toContainText('Teams: from teams.toml (6)');
	// Rev 15: with no line in HR, a row reports to the lead of its top-level team: ada, with no edits.
	await expect(dialog(page).getByLabel('Reports to for server-dev')).toHaveValue('ac-staff-engineer-ada');
	await expect(dialog(page).getByLabel('Reports to for ada')).toHaveValue('owner');
	// The default follows the Team picker until the owner sets a manager by hand.
	await dialog(page).getByLabel('Team for server-dev').selectOption('design');
	await expect(dialog(page).getByLabel('Reports to for server-dev')).toHaveValue('ac-product-designer-eden');
	await dialog(page).getByLabel('Reports to for server-dev').selectOption('owner');
	await dialog(page).getByLabel('Team for server-dev').selectOption('office');
	await expect(dialog(page).getByLabel('Reports to for server-dev')).toHaveValue('owner');
	await expect(dialog(page).getByLabel('Team for Riley')).toHaveValue('app');
	await expect(dialog(page).getByLabel('Team for server-dev')).toHaveValue('office');
	await dialog(page).getByRole('button', { name: 'Edit…' }).click();
	await expect(dialog(page).getByLabel('Lead of App')).toHaveValue('app-frontend-dev');
	await expect(dialog(page).getByLabel('Owner of App')).toHaveValue('engineering');
	await shot(page, 'bring-in-teams');
});

test('IMP-7: Bring in creates the team, highlights it on the chart, and a second run finds nothing new', async ({ page }) => {
	await openEmptyChart(page);
	await scanToReview(page, true);
	for (const name of ['server-dev', 'app-frontend-dev', 'codex-dev', 'agy-senior-dev', 'reviewer']) {
		await dialog(page).getByLabel(`Role for ${name}`).selectOption('software engineer');
		await dialog(page).getByLabel(`Runtime for ${name}`).selectOption(name === 'codex-dev' || name === 'server-dev' ? 'codex' : name === 'agy-senior-dev' ? 'antigravity' : 'claude_code');
	}
	await expect(dialog(page).getByRole('button', { name: /^Bring in 10 agents$/ })).toBeVisible();
	await dialog(page).getByRole('button', { name: /^Bring in 10 agents$/ }).click();
	await expect(dialog(page).getByTestId('bring-in-done')).toContainText('10 agents, 6 teams, 1 project.', { timeout: 30_000 });
	await expect(dialog(page).getByRole('button', { name: 'View Activity' })).toBeVisible();
	await shot(page, 'bring-in-done');
	await dialog(page).getByRole('button', { name: 'View org chart' }).click();
	await expect(page.getByTestId('team-card')).toHaveCount(6);
	await expect(page.locator('[data-member-tile][title^="ada"]')).toContainText('New');
	await shot(page, 'bring-in-chart');
	// The same import again: nothing new, nothing duplicated.
	await page.getByRole('button', { name: 'Bring in a team…' }).click();
	await dialog(page).getByRole('button', { name: 'Scan' }).click();
	await expect(dialog(page).getByTestId('import-note').first()).toContainText('Already in Agentic Consulting.');
	await expect(dialog(page).getByTestId('team-already').first()).toContainText('is already in Agentic Consulting: left as it is.');
	await expect(dialog(page).getByRole('button', { name: /^Bring in 0 agents$/ })).toBeDisabled();
});

test('IMP-7: closing the Done screen by Escape still shows what came in, and only the agents made this time are tagged', async ({ page }) => {
	await openEmptyChart(page);
	await scanToReview(page, true);
	for (const name of ['server-dev', 'app-frontend-dev', 'codex-dev', 'agy-senior-dev', 'reviewer']) {
		await dialog(page).getByLabel(`Role for ${name}`).selectOption('software engineer');
		await dialog(page).getByLabel(`Runtime for ${name}`).selectOption('codex');
	}
	await dialog(page).getByLabel('Bring in reviewer').selectOption('skip');
	await dialog(page).getByRole('button', { name: /^Bring in 9 agents$/ }).click();
	await expect(dialog(page).getByTestId('bring-in-done')).toContainText('9 agents', { timeout: 30_000 });
	await page.keyboard.press('Escape');
	await expect(dialog(page)).toHaveCount(0);
	await expect(page.locator('[data-member-tile][title^="ada"]')).toContainText('New');
	// A second import brings in the one that was left out: only it is tagged.
	await page.getByRole('button', { name: 'Bring in a team…' }).click();
	await dialog(page).getByRole('button', { name: 'Choose a team hints file' }).click();
	await dialog(page).getByRole('button', { name: 'Scan' }).click();
	await dialog(page).getByLabel('Role for reviewer').selectOption('software engineer');
	await dialog(page).getByLabel('Runtime for reviewer').selectOption('codex');
	await dialog(page).getByRole('button', { name: /^Bring in 1 agent$/ }).click();
	await expect(dialog(page).getByTestId('bring-in-done')).toContainText('1 agent', { timeout: 30_000 });
	await dialog(page).getByRole('button', { name: 'View org chart' }).click();
	await expect(page.locator('[data-member-tile][title^="reviewer"]')).toContainText('New');
	await expect(page.locator('[data-member-tile]').filter({ hasText: 'New' })).toHaveCount(1);
});

test('IMP-9: a run that fails part way can be closed, and bringing in again from a new review finishes it', async ({ page }) => {
	await openEmptyChart(page);
	await scanToReview(page, true);
	for (const name of ['server-dev', 'app-frontend-dev', 'codex-dev', 'agy-senior-dev', 'reviewer']) {
		await dialog(page).getByLabel(`Role for ${name}`).selectOption('software engineer');
		await dialog(page).getByLabel(`Runtime for ${name}`).selectOption('codex');
	}
	// The office drops the first lead being set.
	await page.evaluate(() => (window as unknown as { __mockOrgFailNext: (m: string) => void }).__mockOrgFailNext('team.set_lead'));
	await dialog(page).getByRole('button', { name: /^Bring in 10 agents$/ }).click();
	await expect(dialog(page).getByTestId('bring-in-done')).toContainText('Not everything went through', { timeout: 30_000 });
	await expect(dialog(page).getByTestId('bring-in-done')).toContainText('Set the lead of');
	await dialog(page).getByRole('button', { name: 'Close', exact: true }).last().click();
	// A new review: every agent is "Already in", yet there is something to finish.
	await page.getByRole('button', { name: 'Bring in a team…' }).click();
	await dialog(page).getByRole('button', { name: 'Choose a team hints file' }).click();
	await dialog(page).getByRole('button', { name: 'Scan' }).click();
	await expect(dialog(page).getByRole('button', { name: 'Finish bringing in' })).toBeEnabled();
	await dialog(page).getByRole('button', { name: 'Finish bringing in' }).click();
	await expect(dialog(page).getByTestId('bring-in-done')).not.toContainText('Not everything went through', { timeout: 30_000 });
	await dialog(page).getByRole('button', { name: 'Close' }).last().click();
	await page.getByRole('button', { name: 'Bring in a team…' }).click();
	await dialog(page).getByRole('button', { name: 'Choose a team hints file' }).click();
	await dialog(page).getByRole('button', { name: 'Scan' }).click();
	await expect(dialog(page).getByRole('button', { name: /^Bring in 0 agents$/ })).toBeDisabled();
});


test('IMP-1: a new organization lands on Agents, where bringing in an existing team is offered, and it opens the same review', async ({ page }) => {
	await visit(page, '/create');
	await page.getByLabel('Organization name').fill('Fresh Org');
	await page.getByRole('button', { name: 'Create organization' }).click();
	await expect(page).toHaveURL(/\/agents$/);
	await visit(page, '/agents?org=empty');
	await expect(page.getByTestId('no-agents')).toBeVisible();
	await page.getByTestId('bring-in-empty-agents').click();
	await expect(dialog(page).getByTestId('bring-in-sources')).toContainText('Found 5 agents in 1 project (ac)');
	await dialog(page).getByRole('button', { name: 'Scan' }).click();
	await expect(page.getByTestId('import-row')).toHaveCount(10);
	await dialog(page).getByRole('button', { name: 'Cancel' }).click();
	await expect(dialog(page)).toHaveCount(0);
	await page.getByTestId('bring-in-header').click();
	await expect(dialog(page)).toBeVisible();
});
