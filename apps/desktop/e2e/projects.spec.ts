import { expect, test, type Page } from '@playwright/test';
import { visit } from './ready';
import { shot } from './shot';

async function open(page: Page, path = '/projects') {
	await visit(page, '/create');
	await page.getByLabel('Organization name').fill('Acme');
	await page.getByRole('button', { name: 'Create organization' }).click();
	await expect(page).toHaveURL(/\/agents$/);
	await visit(page, path);
}
const row = (page: Page, id: string) => page.locator(`[data-testid=project-row][data-project=${id}]`);

test('PROJ-1: the list shows every project A–Z with its teams, Shared, leads and No team', async ({ page }) => {
	await open(page);
	await expect(page.getByTestId('project-row')).toHaveCount(4);
	const names = await page.getByTestId('project-row').locator('a').allInnerTexts();
	expect(names).toEqual(['Atlas', 'Beacon', 'Onboarding kit', 'Scratchpad']);
	const beacon = row(page, 'beacon');
	await expect(beacon.getByTestId('project-team')).toHaveText([/Design/, /Web/]);
	await expect(beacon).toContainText('Shared');
	await expect(beacon.getByTestId('leads-line')).toHaveText('Leads: Juno Park (Design), Noor Haddad (Web)');
	await expect(row(page, 'atlas').getByTestId('leads-line')).toHaveText('Lead: Kai Lindqvist');
	await expect(row(page, 'scratch').getByTestId('no-team')).toBeVisible();
	await expect(row(page, 'scratch').getByTestId('leads-line')).toHaveText(''); // an unassigned project has no leads line
	await expect(page.getByTestId('unassigned-note')).toContainText('1 project has no team yet.');
	await expect(page.getByRole('button', { name: 'New project' })).toBeDisabled();
	await shot(page, 'projects');
});

test('PROJ-2: Assign team… gives an unassigned project a team; adding a second team shares it', async ({ page }) => {
	await open(page);
	await row(page, 'scratch').getByRole('button', { name: 'Assign a team to Scratchpad' }).click();
	const dialog = page.getByRole('dialog', { name: 'Assign a team to Scratchpad' });
	await dialog.getByRole('radio', { name: 'Design' }).check();
	await dialog.getByRole('button', { name: 'Assign team' }).click();
	await expect(page.getByText('Scratchpad now belongs to Design.')).toBeVisible();
	await expect(row(page, 'scratch').getByTestId('project-team')).toHaveText([/Design/]);
	await expect(page.getByTestId('unassigned-note')).toHaveCount(0);
	await row(page, 'scratch').getByRole('button', { name: 'Assign a team to Scratchpad' }).click();
	await page.getByRole('dialog').getByRole('radio', { name: 'Web' }).check();
	await page.getByRole('dialog').getByRole('button', { name: 'Assign team' }).click();
	await expect(page.getByText('Scratchpad is now shared by Design and Web.')).toBeVisible();
	await expect(row(page, 'scratch')).toContainText('Shared');
});

test('PROJ-2: removing a team needs no confirmation while others remain; the last one asks first', async ({ page }) => {
	await open(page);
	await row(page, 'beacon').getByRole('button', { name: 'Remove Beacon from Web' }).click();
	await expect(page.getByText('Removed Beacon from Web.')).toBeVisible();
	await expect(row(page, 'beacon').getByTestId('project-team')).toHaveText([/Design/]);
	await row(page, 'beacon').getByRole('button', { name: 'Remove Beacon from Design' }).click();
	const dialog = page.getByRole('dialog', { name: 'Remove Beacon from Design?' });
	await expect(dialog).toContainText('It will have no team and show under Unassigned projects.');
	await dialog.getByRole('button', { name: 'Remove' }).click();
	await expect(row(page, 'beacon').getByTestId('no-team')).toBeVisible();
});

test('PROJ-3: a project page shows its name, teams and leads, and Tasks is a tab and the tabs that come later are disabled', async ({ page }) => {
	await open(page);
	await row(page, 'beacon').getByRole('link', { name: 'Beacon' }).click();
	await expect(page).toHaveURL(/\/projects\/beacon$/);
	await expect(page.getByTestId('project-name')).toHaveText('Beacon');
	await expect(page.getByRole('link', { name: 'Projects' }).first()).toBeVisible(); // breadcrumb
	await expect(page.getByTestId('leads-line')).toHaveText('Leads: Juno Park (Design), Noor Haddad (Web)');
	await expect(page.getByRole('tab', { name: 'Overview' })).toHaveAttribute('aria-selected', 'true');
	await expect(page.getByRole('tab', { name: 'Tasks' })).toBeEnabled();
	for (const name of ['Features', 'Bugs', 'Debt', 'Docs', 'Insights', 'Settings']) await expect(page.getByRole('tab', { name })).toHaveAttribute('aria-disabled', 'true');
	await shot(page, 'project-page');
	// Teams are editable from the page too.
	await page.getByRole('button', { name: 'Remove Beacon from Web' }).click();
	await expect(page.getByTestId('leads-line')).toHaveText('Lead: Juno Park');
});

test('PROJ-3: a project that isn’t there says so and links back', async ({ page }) => {
	await open(page, '/projects/ghost');
	await expect(page.getByTestId('project-gone')).toContainText("This project isn't here any more.");
	await page.getByRole('link', { name: 'Go to Projects' }).click();
	await expect(page).toHaveURL(/\/projects$/);
});

test('PROJ-1: an organization with no projects says so', async ({ page }) => {
	await open(page, '/projects?org=empty');
	await expect(page.getByTestId('no-projects')).toContainText('No projects yet');
});
