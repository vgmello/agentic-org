import { expect, test, type Page } from '@playwright/test';
import { startInstance, type Instance } from './instance';
import { removeInstance } from './teardown';

// TASK-40 (U1): live Offices, Teams, Members, Org Chart, Projects, and Access screens
// against a real `agentic-org` host, web gateway and office worker (zero browser mocks).
const PREFIX = 'u1pw';
const PORT_BASE = 24280;
const login = 'alex';
const password = 'correct horse battery';
const org = {
	name: 'Lumen Labs',
	slug: 'lumen-labs',
	repo: 'https://example.test/lumen/state.git'
};

let instance: Instance | undefined;

test.beforeAll(async () => {
	instance = await startInstance(PREFIX, PORT_BASE);
});

test.afterAll(() => {
	removeInstance(instance?.root, PREFIX);
});

const teamTreeLink = (page: Page, name: string) =>
	page
		.getByRole('list', { name: 'Teams, as a tree' })
		.getByRole('link', { name: new RegExp(`^${name}\\b`) });

test('U1: Offices, Teams, People, Org Chart, Projects, and Access work end-to-end against live /api/v1', async ({
	page
}) => {
	const { link, base } = instance!;

	// 1. Register administrator and create an organization, then open it into the shell (/teams).
	await page.goto(link);
	await page.getByLabel('Username').fill(login);
	await page.getByLabel('Password (at least 12 characters)').fill(password);
	await page.getByLabel('Password again').fill(password);
	await page.getByRole('button', { name: 'Create account', exact: true }).click();
	await expect(page).toHaveURL(/\/welcome$/);

	await page.getByRole('link', { name: 'Create organization' }).click();
	await page.getByLabel('Name', { exact: true }).fill(org.name);
	await page.getByLabel('Org Git URL').fill(org.repo);
	await page.getByLabel('State branch').fill('main');
	await page.getByRole('button', { name: 'Create organization', exact: true }).click();
	await expect(
		page.getByRole('heading', { level: 1, name: `Created ${org.name}. You own it.` })
	).toBeVisible({ timeout: 120_000 });

	await page.getByRole('button', { name: `Open ${org.name}`, exact: true }).click();
	await expect(page).toHaveURL(/\/teams$/);
	await expect(page.getByRole('heading', { level: 2, name: /^Teams/ })).toBeVisible();
	await expect(teamTreeLink(page, org.name)).toBeVisible();

	// 2. Offices (/settings/offices): live HQ office Online + details sheet.
	await page.goto(`${base}/settings/offices`);
	await expect(page.getByRole('heading', { level: 2, name: /^Offices/ })).toBeVisible();
	await expect(page.getByText('Online', { exact: true }).first()).toBeVisible();

	// 3. Teams: create child teams (Engineering, Design, Temp), rename Design -> Product Design, delete Temp.
	await page.goto(`${base}/teams`);
	for (const name of ['Engineering', 'Design', 'Temp']) {
		await page.getByRole('button', { name: 'New team', exact: true }).click();
		const dialog = page.getByRole('dialog');
		await dialog.getByLabel('Name', { exact: true }).fill(name);
		await dialog.getByRole('button', { name: 'Create team', exact: true }).click();
		await expect(dialog).toBeHidden();
		await expect(teamTreeLink(page, name)).toBeVisible();
	}

	// Open Engineering team page: lands on Overview with explicit unavailable states (never fake zeros),
	// then switch to Members tab to verify added people (PEOPLE-3, PEOPLE-4).
	await teamTreeLink(page, 'Engineering').click();
	await expect(page).toHaveURL(/\/teams\/[^/]+$/);
	const engTeamUrl = page.url();
	const teamOverview = page.getByTestId('team-overview');
	await expect(teamOverview).toBeVisible();
	await expect(teamOverview.locator('[data-kpi="blocked"]')).toContainText('—');
	await expect(teamOverview.getByTestId('team-work-unavailable')).toBeVisible();
	await expect(teamOverview.getByTestId('team-activity-unavailable')).toBeVisible();

	await page.getByRole('button', { name: 'Add a person', exact: true }).click();
	let dialog = page.getByRole('dialog');
	await dialog.getByLabel('Name', { exact: true }).fill('Ada Lovelace');
	await dialog.getByLabel('Handle', { exact: true }).fill('ada');
	await dialog.getByLabel(/Contact/).fill('ada@lumen.test');
	await dialog.getByRole('button', { name: 'Add person', exact: true }).click();
	await expect(dialog).toBeHidden();
	await page
		.getByRole('navigation', { name: 'Engineering views' })
		.getByRole('link', { name: /^Members\b/ })
		.click();
	await expect(page.getByRole('link', { name: /Ada Lovelace/ })).toBeVisible();

	// Add a second person to test manager assignment, edit, move home team, and offboarding.
	await page.getByRole('button', { name: 'Add a person', exact: true }).click();
	dialog = page.getByRole('dialog');
	await dialog.getByLabel('Name', { exact: true }).fill('Grace Hopper');
	await dialog.getByLabel('Handle', { exact: true }).fill('grace');
	await dialog.getByRole('button', { name: 'Add person', exact: true }).click();
	await expect(dialog).toBeHidden();
	await expect(page.getByRole('link', { name: /Grace Hopper/ })).toBeVisible();

	// Set Ada as lead of Engineering (TEAM-15).
	await page.getByRole('button', { name: 'Actions for Engineering', exact: true }).click();
	await page.getByRole('menuitem', { name: 'Change lead…', exact: true }).click();
	dialog = page.getByRole('dialog');
	await dialog.getByRole('radio', { name: /Ada Lovelace/ }).check();
	await dialog.getByRole('button', { name: /Save|Set lead/i }).click();
	await expect(dialog).toBeHidden();
	await expect(page.locator('main')).toContainText('Ada Lovelace');

	// Rename Design -> Product Design and Delete Temp (TEAM-9).
	await page.goto(`${base}/teams`);
	await teamTreeLink(page, 'Design').click();
	await expect(page).toHaveURL(/\/teams\/[^/]+$/);
	await page.getByRole('button', { name: 'Actions for Design', exact: true }).click();
	await page.getByRole('menuitem', { name: 'Rename…', exact: true }).click();
	dialog = page.getByRole('dialog');
	await dialog.getByLabel('Name', { exact: true }).fill('Product Design');
	await dialog.getByRole('button', { name: 'Rename', exact: true }).click();
	await expect(dialog).toBeHidden();
	await expect(page.getByRole('heading', { level: 2, name: /Product Design/ })).toBeVisible();

	await page.goto(`${base}/teams`);
	await teamTreeLink(page, 'Temp').click();
	await expect(page).toHaveURL(/\/teams\/[^/]+$/);
	await page.getByRole('button', { name: 'Actions for Temp', exact: true }).click();
	await page.getByRole('menuitem', { name: 'Delete…', exact: true }).click();
	dialog = page.getByRole('dialog');
	await dialog.getByRole('button', { name: /^Delete/ }).click();
	await expect(dialog).toBeHidden();
	await expect(page).toHaveURL(/\/teams$/);
	await expect(teamTreeLink(page, 'Temp')).toHaveCount(0);

	// 4. Members directory & profile: edit name/contact (PEOPLE-8), move home team (TEAM-6), remove person (PEOPLE-5).
	await page.goto(`${base}/teams/members`);
	await expect(page.getByRole('heading', { level: 2, name: /^Members/ })).toBeVisible();
	await page.getByRole('link', { name: 'Grace Hopper, person', exact: true }).click();
	await expect(page).toHaveURL(/\/members\/[^/]+$/);

	// Edit Grace's name and contact while keeping @grace handle.
	await page.getByRole('button', { name: /^More/ }).click();
	await page.getByRole('menuitem', { name: 'Edit…', exact: true }).click();
	dialog = page.getByRole('dialog');
	await dialog.getByLabel('Name', { exact: true }).fill('Grace Murray Hopper');
	await dialog.getByLabel(/Contact/).fill('grace@lumen.test');
	await dialog.getByRole('button', { name: 'Save', exact: true }).click();
	await expect(dialog).toBeHidden();
	await expect(page.getByRole('heading', { level: 2, name: /Grace Murray Hopper/ })).toBeVisible();
	await expect(page.locator('main header').first()).toContainText('@grace');
	await expect(page.locator('main header').first()).toContainText('grace@lumen.test');

	// Move Grace's home team from Engineering to Product Design.
	await page.getByRole('button', { name: /^More/ }).click();
	await page.getByRole('menuitem', { name: 'Move home team…', exact: true }).click();
	dialog = page.getByRole('dialog');
	await dialog.getByRole('radio', { name: /^Product Design/ }).check();
	await dialog.getByRole('button', { name: /Move Grace Murray Hopper/ }).click();
	await expect(dialog).toBeHidden();
	await expect(page.locator('main header').first()).toContainText('Home team Product Design');

	// 5. Org chart (/teams/org-chart): set Grace's manager to Ada (REPORT-1..5).
	await page.goto(`${base}/teams/org-chart`);
	await expect(page.getByRole('heading', { level: 2, name: /^Org chart/ })).toBeVisible();
	await expect(page.locator('main')).toContainText('Ada Lovelace');
	await expect(page.locator('main')).toContainText('Grace Murray Hopper');
	await page.getByRole('button', { name: 'Actions for Grace Murray Hopper', exact: true }).click();
	await page.getByRole('menuitem', { name: 'Change manager…', exact: true }).click();
	dialog = page.getByRole('dialog');
	await dialog.getByRole('radio', { name: /Ada Lovelace/ }).check();
	await dialog.getByRole('button', { name: /Save|Change manager/i }).click();
	await expect(dialog).toBeHidden();
	await page.reload();
	await expect(page.locator('main')).toContainText('Ada Lovelace');
	await expect(page.locator('main')).toContainText('Grace Murray Hopper');

	// Remove (deactivate) Grace from her profile (PEOPLE-5).
	await page.goto(`${base}/teams/members`);
	await page.getByRole('link', { name: 'Grace Murray Hopper, person', exact: true }).click();
	await page.getByRole('button', { name: /^More/ }).click();
	await page.getByRole('menuitem', { name: 'Remove person…', exact: true }).click();
	dialog = page.getByRole('dialog');
	await dialog.getByRole('button', { name: 'Remove person', exact: true }).click();
	await expect(dialog).toBeHidden();
	await page.goto(`${base}/teams/members`);
	await expect(
		page.getByRole('link', { name: 'Grace Murray Hopper, person', exact: true })
	).toHaveCount(0);

	// 6. Projects (/projects): create project with existing team, add & remove contributing team (TEAM-8/13/14).
	await page.goto(`${base}/projects`);
	await expect(page.getByRole('heading', { level: 2, name: /^Projects/ })).toBeVisible();
	await page.getByRole('button', { name: 'New project', exact: true }).click();
	dialog = page.getByRole('dialog');
	await dialog.getByLabel('Name', { exact: true }).fill('Checkout');
	await dialog.getByRole('radio', { name: /An existing team/i }).check();
	const teamSelect = dialog.locator('select').first();
	const engValue = await teamSelect
		.locator('option', { hasText: 'Engineering' })
		.first()
		.getAttribute('value');
	await teamSelect.selectOption(engValue!);
	await dialog.getByRole('button', { name: 'Create project', exact: true }).click();
	await expect(dialog).toBeHidden();
	await expect(page).toHaveURL(/\/projects\/[^/]+$/);
	await expect(page.getByRole('heading', { level: 2, name: /Checkout/ })).toBeVisible();

	// Add Product Design as a contributing team on Project Settings, verify on Team tab, then remove it.
	const projectUrl = page.url();
	await page.goto(`${projectUrl}/settings`);
	const teamsSection = page.locator('section[aria-labelledby="settings-teams-heading"]');
	await teamsSection.getByRole('checkbox', { name: /Product Design/ }).check();
	await teamsSection.getByRole('button', { name: 'Save', exact: true }).click();
	await expect(teamsSection.getByRole('status')).toHaveText('Saved');

	await page.goto(`${projectUrl}/team`);
	await expect(page.locator('main')).toContainText('Engineering');
	await expect(page.locator('main')).toContainText('Product Design');

	await page.goto(`${projectUrl}/settings`);
	await teamsSection.getByRole('checkbox', { name: /Product Design/ }).uncheck();
	await teamsSection.getByRole('button', { name: 'Save', exact: true }).click();
	await expect(teamsSection.getByRole('status')).toHaveText('Saved');

	// Verify Engineering Team Overview lists live Checkout project link with explicit progress/health unavailability (never fake "No open tasks").
	await page.goto(engTeamUrl);
	const engOverview = page.getByTestId('team-overview');
	await expect(engOverview.getByRole('link', { name: 'Checkout', exact: true })).toBeVisible();
	await expect(engOverview).toContainText("Progress and health aren't served by this server yet.");
	await expect(engOverview).not.toContainText('No open tasks');

	// 7. Access (/settings/access): live people list and issue one-time invite link for Ada.
	await page.goto(`${base}/settings/access`);
	await expect(
		page.getByRole('heading', { level: 2, name: 'Members and access', exact: true })
	).toBeVisible();
	const adaRow = page.locator('[data-access-row]').filter({ hasText: 'Ada Lovelace' });
	await expect(adaRow).toBeVisible();
	await adaRow.getByRole('button', { name: 'Invite to sign in', exact: true }).click();
	dialog = page.getByRole('dialog');
	await expect(dialog).toContainText('Invite Ada Lovelace to sign in');
	await expect(dialog.locator('input')).toHaveValue(/\/register#/);
	await dialog.getByRole('button', { name: 'Done', exact: true }).click();
	await expect(dialog).toBeHidden();
});
