import { expect, test, type Page } from '@playwright/test';
import { visit } from './ready';
import { shot } from './shot';

async function openOffices(page: Page, query = '') {
	await visit(page, '/create');
	await page.getByLabel('Organization name').fill('Acme');
	await page.getByRole('button', { name: 'Create organization' }).click();
	await expect(page).toHaveURL(/\/agents$/);
	await visit(page, `/offices${query}`);
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
	await expect(page.getByTestId('sample-data-note')).toContainText('simulated');
	await expect(page.getByTestId('add-hint')).toContainText('Add an office to run agents on another Mac');
	await expect(page.getByTestId('unplaced-note')).toContainText('1 agent is not placed in an office: Inês Duarte');
	await shot(page, 'offices');
});

test('OFC-1: a row opens a slide-over with the office’s details', async ({ page }) => {
	await openOffices(page);
	await rows(page).first().getByRole('button').click();
	const details = page.getByTestId('office-details');
	await expect(details).toContainText('HQ');
	await expect(details).toContainText('This Mac');
	await expect(details).toContainText('Online');
	await expect(details).toContainText('Up to date');
	await expect(details).toContainText('Sam Okafor'); // an agent placed here
	await expect(page.getByRole('button', { name: /Remove office/ })).toHaveCount(0); // HQ can't be removed
	await shot(page, 'offices-details');
});

test('OFC-2: Add office › This Mac creates and starts the office with no link or approval', async ({ page }) => {
	await openOffices(page);
	await page.getByRole('button', { name: 'Add office' }).click();
	await expect(page.getByLabel('Name')).toHaveValue('Office 2');
	await expect(page.getByRole('radio', { name: /Another computer/ })).toBeDisabled();
	await expect(page.getByRole('radio', { name: /This Mac/ })).toBeChecked();
	await shot(page, 'offices-add');
	await page.getByRole('button', { name: 'Continue' }).click();
	await expect(page.getByTestId('add-steps')).toContainText('Creating Office 2');
	await expect(page.getByTestId('add-steps')).toContainText('Copying records');
	await expect(page.getByRole('heading', { name: 'Add office' })).toHaveCount(0, { timeout: 15_000 }); // closes by itself
	await expect(rows(page)).toHaveCount(2);
	await expect(rows(page).nth(1)).toContainText('Office 2');
	await expect(rows(page).nth(1)).toContainText('Online');
	await expect(rows(page).nth(1)).toContainText('0 agents');
	await expect(page.getByTestId('add-hint')).toHaveCount(0); // not alone any more
});

test('OFC-2: the name is required, unique among the organization’s offices, and shows the error inline', async ({ page }) => {
	await openOffices(page);
	await page.getByRole('button', { name: 'Add office' }).click();
	await page.getByLabel('Name').fill('   ');
	await page.getByRole('button', { name: 'Continue' }).click();
	await expect(page.getByRole('alert')).toHaveText('Give the office a name.');
	await page.getByLabel('Name').fill('hq');
	await page.getByRole('button', { name: 'Continue' }).click();
	await expect(page.getByRole('alert')).toContainText("There's already an office called HQ");
	await page.getByLabel('Name').fill('Studio Mac');
	await expect(page.getByRole('alert')).toHaveCount(0); // typing clears the error
});

test('OFC-2: a failed start leaves the row as Stopped, with the reason', async ({ page }) => {
	await openOffices(page, '?mock=office_start_fails');
	await page.getByRole('button', { name: 'Add office' }).click();
	await page.getByRole('button', { name: 'Continue' }).click();
	await expect(page.getByText("Office 2 couldn't start: The office server didn't answer in time.")).toBeVisible({ timeout: 15_000 });
	await shot(page, 'offices-start-failed');
	await page.getByRole('dialog').getByRole('button', { name: 'Close' }).last().click();
	const row = rows(page).filter({ hasText: 'Office 2' });
	await expect(row).toContainText('Stopped');
	await expect(row).toContainText("The office server didn't answer in time."); // the reason is on the row itself
	await row.getByRole('button').first().click();
	await expect(page.getByTestId('office-details')).toContainText("The office server didn't answer in time.");
});

test('OFC-8: removing an office names what happens, and removes it only after the confirmation', async ({ page }) => {
	await openOffices(page);
	await page.getByRole('button', { name: 'Add office' }).click();
	await page.getByRole('button', { name: 'Continue' }).click();
	await expect(rows(page)).toHaveCount(2, { timeout: 15_000 });

	await rows(page).nth(1).getByRole('button').first().click();
	await page.getByRole('button', { name: 'Remove office…' }).click();
	const dialog = page.getByRole('alertdialog');
	await expect(dialog).toContainText('Remove Office 2 from Acme?');
	await expect(dialog).toContainText("Office 2 can't sync again");
	await expect(dialog).toContainText('No agents run there');
	await expect(dialog).toContainText('If Office 2 is offline when you remove it'); // OFC-8: the offline case is always explained
	await expect(dialog).toContainText('You can add it to Acme again later');
	await expect(dialog.getByTestId('sample-data-note-remove')).toBeVisible();
	await shot(page, 'offices-remove');

	await dialog.getByRole('button', { name: 'Cancel' }).click();
	await expect(rows(page)).toHaveCount(2); // nothing happened
	await page.getByRole('button', { name: 'Remove office…' }).click();
	await page.getByRole('alertdialog').getByRole('button', { name: 'Remove Office 2' }).click();
	await expect(rows(page)).toHaveCount(1);
	await expect(page.getByTestId('add-hint')).toBeVisible();
});

test('OFC-2: after a failed start, Copy details and Try again are offered; Try again restarts that office instead of adding another', async ({ page }) => {
	await openOffices(page, '?mock=office_start_fails');
	await page.getByRole('button', { name: 'Add office' }).click();
	await page.getByRole('button', { name: 'Continue' }).click();
	await expect(page.getByText("Office 2 couldn't start")).toBeVisible({ timeout: 15_000 });
	await expect(page.getByRole('button', { name: 'Copy details' })).toBeVisible();
	await page.getByRole('button', { name: 'Try again' }).click();
	await expect(page.getByText("Office 2 couldn't start")).toBeVisible({ timeout: 15_000 }); // it fails again in this scenario
	await page.getByRole('dialog').getByRole('button', { name: 'Close' }).last().click();
	await expect(rows(page)).toHaveCount(2); // still one Office 2, not two
});

test('a long office name wraps in the remove dialog and the confirm button stays on screen', async ({ page }) => {
	await openOffices(page);
	await page.getByRole('button', { name: 'Add office' }).click();
	await page.getByLabel('Name').fill('W'.repeat(64));
	await page.getByRole('button', { name: 'Continue' }).click();
	await expect(rows(page)).toHaveCount(2, { timeout: 15_000 });
	await rows(page).nth(1).getByRole('button').first().click();
	await page.getByRole('button', { name: 'Remove office…' }).click();
	const confirm = page.getByRole('alertdialog').getByRole('button', { name: /^Remove W+$/ });
	await expect(confirm).toBeInViewport();
	const box = (await page.getByRole('alertdialog').boundingBox())!;
	expect(box.width).toBeLessThanOrEqual(1440);
});

test('the Add office sheet says adding is simulated, and a long name wraps in the progress and failure messages', async ({ page }) => {
	await openOffices(page, '?mock=office_start_fails');
	await page.getByRole('button', { name: 'Add office' }).click();
	await expect(page.getByTestId('sample-data-note-add')).toContainText('Sample data');
	await page.getByLabel('Name').fill('W'.repeat(64));
	await page.getByRole('button', { name: 'Continue' }).click();
	await expect(page.getByRole('alert')).toContainText("couldn't start", { timeout: 15_000 });
	await expect(page.getByLabel('Name')).toBeDisabled(); // Try again restarts that office; the name can't change now
	const sheet = await page.getByRole('dialog').boundingBox();
	for (const el of [page.getByTestId('add-steps'), page.getByRole('alert')]) {
		const box = (await el.boundingBox())!;
		expect(box.x + box.width).toBeLessThanOrEqual(sheet!.x + sheet!.width + 1);
	}
	await shot(page, 'offices-add-long-name');
});

test('a name that is already taken is fixed in the field, not shown as a Stopped office', async ({ page }) => {
	await openOffices(page);
	await page.getByRole('button', { name: 'Add office' }).click();
	await page.getByLabel('Name').fill('Office 2');
	await page.getByRole('button', { name: 'Continue' }).click();
	await expect(rows(page)).toHaveCount(2, { timeout: 15_000 });
	await page.getByRole('button', { name: 'Add office' }).click();
	await page.getByLabel('Name').fill('office 2');
	await page.getByRole('button', { name: 'Continue' }).click();
	await expect(page.getByRole('alert')).toContainText('already an office called');
	await expect(page.getByRole('button', { name: 'Try again' })).toHaveCount(0);
});
