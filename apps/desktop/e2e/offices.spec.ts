import { expect, test, type Page } from '@playwright/test';
import { visit } from './ready';
import { shot } from './shot';

async function openOffices(page: Page) {
	await visit(page, '/create');
	await page.getByLabel('Organization name').fill('Acme');
	await page.getByRole('button', { name: 'Create organization' }).click();
	await expect(page).toHaveURL(/\/agents$/);
	await visit(page, '/offices');
	await expect(page.getByRole('list', { name: 'Offices' })).toBeVisible();
}
const rows = (page: Page) => page.getByTestId('office-row');

test('OFC-1: the list shows HQ first with its status, where it runs, and how many agents', async ({ page }) => {
	await openOffices(page);
	await expect(rows(page)).toHaveCount(1);
	const hq = rows(page).first();
	await expect(hq).toContainText('HQ');
	await expect(hq).toContainText('Online');
	await expect(hq).toContainText("This Mac (you're here)");
	await expect(hq).toContainText('7 agents');
	await expect(page.getByTestId('unplaced-note')).toContainText('1 agent is not placed in an office: Inês Duarte');
	await shot(page, 'offices');
});

test('the agent counts say they are sample data', async ({ page }) => {
	await openOffices(page);
	await expect(page.getByTestId('sample-data-note')).toContainText('Sample data');
});

test('OFC-1: a row opens a slide-over with the office’s details', async ({ page }) => {
	await openOffices(page);
	await rows(page).first().getByRole('button').click();
	const details = page.getByTestId('office-details');
	await expect(details).toContainText('HQ');
	await expect(details).toContainText('This Mac');
	await expect(details).toContainText('Online');
	await expect(details).toContainText('Sam Okafor'); // an agent placed here
	await expect(page.getByTestId('sample-data-note-sheet')).toContainText('Sample data'); // the sheet covers the page's note
	await expect(details).toContainText("This Mac (you're here)");
	await expect(page.getByRole('button', { name: /Remove office/ })).toHaveCount(0); // phase 1 has HQ only
	await shot(page, 'offices-details');
});

test('phase 1: Add office is disabled and says it comes in a later release', async ({ page }) => {
	await openOffices(page);
	const add = page.getByRole('button', { name: 'Add office' });
	await expect(add).toBeDisabled();
	await expect(add).toHaveAccessibleDescription('Available in a later release.');
});
