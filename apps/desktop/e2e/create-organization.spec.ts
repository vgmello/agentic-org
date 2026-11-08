import { expect, test, type Page } from '@playwright/test';
import { shot } from './shot';

async function createOrg(page: Page, name: string) {
	await page.getByLabel('Organization name').fill(name);
	await page.getByRole('button', { name: /create organization|try again/i }).click();
}

test('first launch: welcome → create → shell, then straight into the shell', async ({ page }) => {
	await page.goto('/');
	await expect(page).toHaveURL(/\/welcome$/);
	await expect(page.getByRole('heading', { name: 'Welcome to agentic.org' })).toBeVisible();
	await expect(page.getByTestId('organization-row')).toHaveCount(2);
	await shot(page, 'welcome');

	await page.getByRole('link', { name: /Create organization/ }).click();
	await expect(page).toHaveURL(/\/create$/);
	await createOrg(page, '  Acme Studio  ');
	await expect(page.getByText('Start the office on This Mac')).toBeVisible();
	await shot(page, 'create-starting');

	await expect(page).toHaveURL(/\/agents$/);
	await expect(page.getByText('Acme Studio', { exact: true })).toBeVisible();
	await shot(page, 'shell-empty');
	await expect(page.getByText('No agents yet')).toBeVisible();
	await expect(page.getByRole('button', { name: 'Hire your first agent' })).toBeDisabled();

	await page.goto('/');
	await expect(page).toHaveURL(/\/agents$/);
});

test('an empty or duplicate name stays on the form with a message', async ({ page }) => {
	await page.goto('/create');
	await page.getByRole('button', { name: 'Create organization' }).click();
	await expect(page.getByRole('alert')).toHaveText('Give the organization a name.');
	await shot(page, 'create-name-error');

	await createOrg(page, 'Acme');
	await expect(page).toHaveURL(/\/agents$/);
	await page.goto('/create');
	await createOrg(page, 'aCME');
	await expect(page.getByRole('alert')).toHaveText('An organization with that name already exists.');
	await expect(page).toHaveURL(/\/create$/);
});

const failures = [
	['not_installed', "The office isn't installed"],
	['needs_approval', 'Approval needed'],
	['unreachable', "The office isn't answering"],
	['storage_error', "Couldn't create the organization"]
] as const;

for (const [scenario, title] of failures) {
	test(`${scenario} shows "${title}" and does not enter the shell`, async ({ page }) => {
		await page.goto(`/create?mock=${scenario}`);
		await createOrg(page, 'Acme');
		await expect(page.getByText(title)).toBeVisible();
		await shot(page, `create-failed-${scenario}`);
		await expect(page.getByRole('button', { name: 'Try again' })).toBeEnabled();
		await expect(page).toHaveURL(/\/create/);
		await page.goto('/');
		await expect(page).toHaveURL(/\/welcome$/);
	});
}

test('the form cannot be left while the office is starting', async ({ page }) => {
	await page.goto('/welcome');
	await page.getByRole('link', { name: /Create organization/ }).click();
	await page.getByLabel('Organization name').fill('Acme');
	await page.getByRole('button', { name: 'Create organization' }).click();
	await expect(page.getByText('Start the office on This Mac')).toBeVisible();

	const back = page.getByRole('link', { name: 'Back' });
	await expect(back).toHaveAttribute('aria-disabled', 'true');
	await expect(back).toHaveAttribute('tabindex', '-1');
	await page.goBack();
	await expect(page).toHaveURL(/\/create$/);

	// The flow's own navigation still goes through.
	await expect(page).toHaveURL(/\/agents$/);
});

test('the name error clears as soon as the owner types', async ({ page }) => {
	await page.goto('/create');
	await page.getByRole('button', { name: 'Create organization' }).click();
	await expect(page.getByRole('alert')).toBeVisible();
	await page.getByLabel('Organization name').fill('A');
	await expect(page.getByRole('alert')).toHaveCount(0);
});

test('the shell shows the most recently created organization', async ({ page }) => {
	await page.goto('/create');
	await createOrg(page, 'Acme');
	await expect(page).toHaveURL(/\/agents$/);
	await page.goto('/create');
	await createOrg(page, 'Beta');
	await expect(page).toHaveURL(/\/agents$/);
	await expect(page.getByText('Beta', { exact: true })).toBeVisible();
	await page.reload();
	await expect(page.getByText('Beta', { exact: true })).toBeVisible();
});
