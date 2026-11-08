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
const card = (page: Page, id: string) => page.locator(`[data-testid=team-card][data-team=${id}]`);
const menu = (page: Page, team: string) => page.getByRole('button', { name: `Actions for ${team}` });

test('TEAM-5: New team takes a name, a Part of team, members and a lead chosen from them; the owner is never offered', async ({ page }) => {
	await openChart(page);
	await page.getByRole('button', { name: 'New team' }).click();
	const sheet = page.getByRole('dialog', { name: 'New team' });
	await expect(sheet).toBeVisible();
	await shot(page, 'new-team');
	// The owner is never offered as a member.
	await expect(sheet.getByRole('checkbox', { name: /^You,/ })).toHaveCount(0);
	// The lead can only be one of the chosen members.
	await expect(sheet.getByLabel('Lead')).toBeDisabled();
	await sheet.getByLabel('Name').fill('QA');
	await sheet.getByLabel('Part of').selectOption({ label: 'Engineering' });
	await sheet.getByRole('checkbox', { name: /^Quinn Adeyemi/ }).click();
	await sheet.getByRole('checkbox', { name: /^Mira Castillo/ }).click();
	await expect(sheet.getByLabel('Lead').locator('option')).toHaveText(['No lead', 'Mira Castillo', 'Quinn Adeyemi']);
	await sheet.getByLabel('Lead').selectOption({ label: 'Quinn Adeyemi' });
	await sheet.getByRole('button', { name: 'Create team' }).click();
	await expect(sheet).toBeHidden();
	await expect(page.getByText('Created QA.')).toBeVisible();
	const qa = page.locator('[data-testid=team-card]', { hasText: 'QA' }).filter({ hasText: 'Lead: Quinn Adeyemi' });
	await expect(qa).toHaveCount(1);
	await expect(qa.getByRole('button', { name: /^Quinn Adeyemi/ })).toBeVisible();
	await expect(qa.getByRole('button', { name: /^Mira Castillo/ })).toBeVisible();
	await expect(card(page, 'eng')).toContainText('3 teams'); // QA is under Engineering now
	// It is still there after a reload: the change was kept.
	await page.reload();
	await expect(page.locator('[data-testid=team-card]', { hasText: 'QA' })).toHaveCount(1);
});

test('TEAM-5: a duplicate sibling name shows the inline error and creates nothing; the same name elsewhere is fine', async ({ page }) => {
	await openChart(page);
	await page.getByRole('button', { name: 'New team' }).click();
	const sheet = page.getByRole('dialog', { name: 'New team' });
	await sheet.getByLabel('Name').fill('design');
	await sheet.getByRole('button', { name: 'Create team' }).click();
	await expect(sheet.getByRole('alert')).toHaveText("There's already a top-level team called design.");
	await sheet.getByLabel('Part of').selectOption({ label: 'Engineering' });
	await sheet.getByLabel('Name').fill('Web');
	await sheet.getByRole('button', { name: 'Create team' }).click();
	await expect(sheet.getByRole('alert')).toHaveText('Engineering already has a team called Web.');
	await sheet.getByLabel('Name').fill('   ');
	await sheet.getByRole('button', { name: 'Create team' }).click();
	await expect(sheet.getByRole('alert')).toHaveText('Name a team (1–64 characters).');
	await sheet.getByLabel('Part of').selectOption({ label: 'Design' });
	await sheet.getByLabel('Name').fill('Web');
	await sheet.getByRole('button', { name: 'Create team' }).click();
	await expect(sheet).toBeHidden();
	await expect(page.locator('[data-testid=team-card]', { hasText: 'Web' })).toHaveCount(2);
});

test('TEAM-5: Add team under… prefills Part of', async ({ page }) => {
	await openChart(page);
	await menu(page, 'Platform').click();
	await page.getByRole('menuitem', { name: 'Add team under Platform…' }).click();
	const sheet = page.getByRole('dialog', { name: 'New team' });
	await expect(sheet.getByLabel('Part of')).toHaveValue('platform');
});

test('TEAM-5: the empty organization offers Create your first team', async ({ page }) => {
	await visit(page, '/create');
	await page.getByLabel('Organization name').fill('Acme');
	await page.getByRole('button', { name: 'Create organization' }).click();
	await expect(page).toHaveURL(/\/agents$/);
	await visit(page, '/org-chart?org=empty');
	await expect(page.getByText('A team can be part of another team')).toBeVisible();
	await page.getByRole('button', { name: 'Create your first team' }).click();
	await expect(page.getByRole('dialog', { name: 'New team' })).toBeVisible();
});

test('TEAM-9: deleting a team that owns teams is refused up front, naming them, with no Delete button', async ({ page }) => {
	await openChart(page);
	await menu(page, 'Engineering').click();
	await page.getByRole('menuitem', { name: 'Delete…' }).click();
	const dialog = page.getByRole('dialog');
	await expect(dialog).toContainText("Can't delete Engineering");
	await expect(dialog).toContainText('Engineering still has 2 teams under it (Web, Platform). Move or delete them first.');
	await expect(dialog.getByRole('button', { name: /^Delete / })).toHaveCount(0);
	await dialog.getByRole('button', { name: 'Close' }).first().click();
	await expect(card(page, 'eng')).toBeVisible();
});

test('TEAM-9: deleting another team names who moves to Not in a team and which project is unassigned, then does it', async ({ page }) => {
	await openChart(page);
	await menu(page, 'People').click();
	await page.getByRole('menuitem', { name: 'Delete…' }).click();
	const dialog = page.getByRole('dialog');
	await expect(dialog).toContainText('Delete People?');
	await expect(dialog).toContainText('Hana Weiss, Inês Duarte and Leo Brandt move to Not in a team.');
	await expect(dialog).toContainText('Onboarding kit has no other team, so it moves to Unassigned projects.');
	await shot(page, 'delete-team');
	await dialog.getByRole('button', { name: 'Cancel' }).click();
	await expect(card(page, 'people')).toBeVisible(); // nothing happened
	await menu(page, 'People').click();
	await page.getByRole('menuitem', { name: 'Delete…' }).click();
	await page.getByRole('dialog').getByRole('button', { name: 'Delete People' }).click();
	await expect(page.getByText('Deleted People.')).toBeVisible();
	await expect(card(page, 'people')).toHaveCount(0);
	await expect(page.getByTestId('not-in-a-team').getByRole('button', { name: /^Hana Weiss/ })).toBeVisible();
	await expect(page.getByTestId('unassigned-projects')).toContainText('Onboarding kit');
});

test('TEAM-5: Rename checks the name like New team, and keeps the team’s members', async ({ page }) => {
	await openChart(page);
	await menu(page, 'Web').click();
	await page.getByRole('menuitem', { name: 'Rename…' }).click();
	const dialog = page.getByRole('dialog', { name: 'Rename Web' });
	await expect(dialog.getByLabel('Name')).toHaveValue('Web');
	await dialog.getByLabel('Name').fill('platform');
	await dialog.getByRole('button', { name: 'Rename' }).click();
	await expect(dialog.getByRole('alert')).toHaveText('Engineering already has a team called platform.');
	await dialog.getByLabel('Name').fill('Frontend');
	await dialog.getByRole('button', { name: 'Rename' }).click();
	await expect(page.getByText('Renamed to Frontend.')).toBeVisible();
	await expect(card(page, 'web')).toContainText('Frontend');
	await expect(card(page, 'web').getByRole('button', { name: /^Noor Haddad/ })).toBeVisible();
});

test('the team menu works from the keyboard', async ({ page }) => {
	await openChart(page);
	await menu(page, 'Design').focus();
	await page.keyboard.press('Enter');
	await expect(page.getByRole('menuitem', { name: 'Rename…' })).toBeVisible();
	await page.keyboard.press('ArrowDown');
	await page.keyboard.press('Enter'); // Add team under Design…
	await expect(page.getByLabel('Part of')).toHaveValue('design');
});

test('the list view has the same team menus', async ({ page }) => {
	await openChart(page);
	await page.getByRole('button', { name: 'List' }).click();
	await expect(menu(page, 'Web')).toBeVisible();
});
