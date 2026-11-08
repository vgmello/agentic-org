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
	// Arrow down to "Add team under Design…" (the menu has several items) and choose it with Enter.
	const target = page.getByRole('menuitem', { name: /^Add team under Design/ });
	for (let i = 0; i < 8 && !(await target.evaluate((el) => el === document.activeElement)); i++) await page.keyboard.press('ArrowDown');
	await page.keyboard.press('Enter');
	await expect(page.getByLabel('Part of')).toHaveValue('design');
});

test('the list view has the same team menus', async ({ page }) => {
	await openChart(page);
	await page.getByRole('button', { name: 'List' }).click();
	await expect(menu(page, 'Web')).toBeVisible();
});

test('TEAM-7: Move to… never offers a team and its sub-teams, names what moves, and offers Undo', async ({ page }) => {
	await openChart(page);
	await menu(page, 'Engineering').click();
	await page.getByRole('menuitem', { name: 'Move to…' }).click();
	const dialog = page.getByRole('dialog', { name: 'Move Engineering to…' });
	// Itself and the teams under it are listed, disabled, with the reason.
	for (const name of ['Engineering', 'Web', 'Platform']) await expect(dialog.getByRole('radio', { name: new RegExp(`^${name}`) })).toBeDisabled();
	await expect(dialog.getByText("Can't move a team under itself or its own teams").first()).toBeVisible();
	await expect(dialog.getByRole('radio', { name: /^None: top level/ })).toBeDisabled(); // already at the top level
	await expect(dialog.getByRole('button', { name: 'Move Engineering' })).toBeDisabled();
	await dialog.getByRole('radio', { name: /^Design/ }).check();
	await expect(dialog.getByTestId('move-summary')).toContainText('Move Engineering (with 2 teams and 2 projects) from the top level to Design? Members and projects stay with it.');
	await shot(page, 'move-team');
	await dialog.getByRole('button', { name: 'Move Engineering' }).click();
	await expect(page.getByText('Moved Engineering to Design.')).toBeVisible();
	// Engineering is now under Design: the sub-team count shows on Design.
	await expect(card(page, 'design').getByRole('button', { name: /^1 team: / })).toBeVisible();
	await page.getByRole('button', { name: 'Undo' }).click();
	await expect(page.getByText('Moved Engineering back.')).toBeVisible();
	await expect(card(page, 'design').getByRole('button', { name: /^1 team: / })).toHaveCount(0);
});

test('TEAM-7: a team can be moved to the top level', async ({ page }) => {
	await openChart(page);
	await menu(page, 'Web').click();
	await page.getByRole('menuitem', { name: 'Move to…' }).click();
	const dialog = page.getByRole('dialog', { name: 'Move Web to…' });
	await expect(dialog.getByText('Web is already here')).toBeVisible(); // under Engineering
	await dialog.getByRole('radio', { name: 'None: top level' }).check();
	await dialog.getByRole('button', { name: 'Move Web' }).click();
	await expect(page.getByText('Moved Web to the top level.')).toBeVisible();
	await expect(card(page, 'eng').getByRole('button', { name: /^1 team: / })).toBeVisible();
});

test('TEAM-6: Edit members adds and removes members in one go, and sets the lead from them', async ({ page }) => {
	await openChart(page);
	await menu(page, 'Web').click();
	await page.getByRole('menuitem', { name: 'Edit members…' }).click();
	const dialog = page.getByRole('dialog', { name: 'Members of Web' });
	await expect(dialog.getByRole('checkbox', { name: /^Noor Haddad/ })).toBeChecked();
	await expect(dialog.getByRole('checkbox', { name: /^You,/ })).toHaveCount(0); // never the owner
	await dialog.getByRole('checkbox', { name: /^Quinn Adeyemi/ }).click();
	await dialog.getByLabel('Lead').selectOption({ label: 'Quinn Adeyemi' });
	await shot(page, 'edit-members');
	await dialog.getByRole('button', { name: 'Save' }).click();
	await expect(page.getByText('Updated the members of Web.')).toBeVisible();
	const web = card(page, 'web');
	await expect(web.getByRole('button', { name: /^Quinn Adeyemi/ })).toBeVisible();
	await expect(web).toContainText('Lead: Quinn Adeyemi');
});

test('TEAM-6: unchecking the lead warns that the lead is cleared, and then clears it', async ({ page }) => {
	await openChart(page);
	await menu(page, 'Web').click();
	await page.getByRole('menuitem', { name: 'Edit members…' }).click();
	const dialog = page.getByRole('dialog', { name: 'Members of Web' });
	await expect(dialog.getByTestId('clears-lead')).toHaveCount(0);
	await dialog.getByRole('checkbox', { name: /^Noor Haddad/ }).click(); // Noor leads Web
	await expect(dialog.getByTestId('clears-lead')).toHaveText(/Noor Haddad leads Web. Removing Noor Haddad also clears Web's lead./);
	await dialog.getByRole('button', { name: 'Remove and clear lead' }).click();
	await expect(page.getByText('Updated the members of Web.')).toBeVisible();
	await expect(card(page, 'web')).toContainText('No lead');
	await expect(card(page, 'web').getByRole('button', { name: /^Noor Haddad/ })).toHaveCount(0);
	// Noor is still in Design: removing her from one team doesn't remove her from the others.
	await expect(card(page, 'design').getByRole('button', { name: /^Noor Haddad/ })).toBeVisible();
});

test('TEAM-6: someone in other teams shows where, and Cancel changes nothing', async ({ page }) => {
	await openChart(page);
	await menu(page, 'Web').click();
	await page.getByRole('menuitem', { name: 'Edit members…' }).click();
	const dialog = page.getByRole('dialog', { name: 'Members of Web' });
	await expect(dialog.getByText('Also in: Design', { exact: false }).first()).toBeVisible();
	await dialog.getByRole('checkbox', { name: /^Quinn Adeyemi/ }).click();
	await dialog.getByRole('button', { name: 'Cancel' }).click();
	await expect(card(page, 'web').getByRole('button', { name: /^Quinn Adeyemi/ })).toHaveCount(0);
});

test('TEAM-8: Add project… from a team shows each project’s current teams; adding a second team shares it with no confirmation', async ({ page }) => {
	await openChart(page);
	await menu(page, 'Web').click();
	await page.getByRole('menuitem', { name: 'Add project…' }).click();
	const dialog = page.getByRole('dialog', { name: 'Add a project to Web' });
	await expect(dialog).toContainText('Teams: Platform'); // Atlas
	await expect(dialog).toContainText('No team'); // Scratchpad
	await expect(dialog.getByRole('radio', { name: /^Beacon/ })).toHaveCount(0); // Web already works on it
	await dialog.getByRole('radio', { name: /^Atlas/ }).check();
	await shot(page, 'add-project');
	await dialog.getByRole('button', { name: 'Add project' }).click();
	await expect(page.getByText('Atlas is now shared by Platform and Web.')).toBeVisible();
	await expect(card(page, 'web').getByRole('list', { name: 'Projects of Web' })).toContainText('Atlas');
	await expect(card(page, 'web').getByRole('button', { name: /Shared project Atlas/ })).toBeVisible();
});

test('TEAM-8: an unassigned project is given a team with Assign team…, and the note goes away', async ({ page }) => {
	await openChart(page);
	await expect(page.getByTestId('unassigned-projects')).toContainText('Scratchpad');
	await page.getByRole('button', { name: 'Assign a team to Scratchpad' }).click();
	const dialog = page.getByRole('dialog', { name: 'Assign a team to Scratchpad' });
	await expect(dialog.getByRole('button', { name: 'Assign team' })).toBeDisabled(); // nothing chosen yet
	await dialog.getByRole('radio', { name: 'Design' }).check();
	await dialog.getByRole('button', { name: 'Assign team' }).click();
	await expect(page.getByText('Scratchpad now belongs to Design.')).toBeVisible();
	await expect(page.getByTestId('unassigned-projects')).toHaveCount(0);
	await expect(page.getByTestId('unassigned-note')).toHaveCount(0);
});

test('TEAM-8: removing a team from a shared project needs no confirmation; removing the last one asks first', async ({ page }) => {
	await openChart(page);
	// Beacon is shared by Web and Design: no confirmation.
	await card(page, 'web').getByRole('button', { name: 'Remove Beacon from Web' }).click();
	await expect(page.getByText('Removed Beacon from Web.')).toBeVisible();
	await expect(card(page, 'web').getByRole('list', { name: 'Projects of Web' })).toHaveCount(0);
	// Now Design is its only team: the removal asks.
	await card(page, 'design').getByRole('button', { name: 'Remove Beacon from Design' }).click();
	const dialog = page.getByRole('dialog', { name: 'Remove Beacon from Design?' });
	await expect(dialog).toContainText('It will have no team and show under Unassigned projects.');
	await dialog.getByRole('button', { name: 'Cancel' }).click();
	await expect(card(page, 'design')).toContainText('Beacon'); // nothing happened
	await card(page, 'design').getByRole('button', { name: 'Remove Beacon from Design' }).click();
	await page.getByRole('dialog').getByRole('button', { name: 'Remove' }).click();
	await expect(page.getByTestId('unassigned-projects')).toContainText('Beacon');
});
