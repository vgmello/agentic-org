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
	await expect(page.getByTestId('unplaced-note')).toContainText('Unplaced · 1 agent');
	await expect(page.getByTestId('unplaced-note')).toContainText('Inês Duarte');
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

test('OFC-13: a row’s label says its name, HQ, where it runs, the status and the agents, and that it opens details', async ({ page }) => {
	await openOffices(page);
	const button = rows(page).first().getByRole('button');
	await expect(button).toHaveAccessibleName("HQ, HQ, This Mac (you're here), Online, 7 agents");
	await expect(button).toHaveAttribute('aria-haspopup', 'dialog');
});

test('OFC-13: every status shows its icon and its word (the word is never replaced by the icon)', async ({ page }) => {
	await openOffices(page);
	const badge = rows(page).first().locator('[data-status-icon]');
	await expect(badge).toHaveAttribute('data-status-icon', 'circle'); // Online
	await expect(badge).toContainText('Online');
});

test('OFC-1: the list opens from the keyboard and closes back to the row', async ({ page }) => {
	await openOffices(page);
	const button = rows(page).first().getByRole('button');
	await button.focus();
	await page.keyboard.press('Enter');
	await expect(page.getByTestId('office-details')).toBeVisible();
	await page.keyboard.press('Escape');
	await expect(page.getByTestId('office-details')).toBeHidden();
	await expect(button).toBeFocused();
});

test('Unplaced agents are a group with a disabled Place… that says why', async ({ page }) => {
	await openOffices(page);
	const group = page.getByTestId('unplaced-note');
	await expect(group.getByRole('heading', { name: 'Unplaced · 1 agent' })).toBeVisible();
	const place = group.getByRole('button', { name: 'Place…' });
	await expect(place).toBeDisabled();
	await expect(place).toHaveAccessibleDescription('Placing agents is available in a later release.');
});

test('a failure to count agents keeps the list and shows an in-page error with Retry, not an error page', async ({ page }) => {
	await visit(page, '/create');
	await page.getByLabel('Organization name').fill('Acme');
	await page.getByRole('button', { name: 'Create organization' }).click();
	await expect(page).toHaveURL(/\/agents$/);
	await visit(page, '/offices?org=fails');
	await expect(rows(page)).toHaveCount(1); // the list is there
	await expect(page.getByTestId('count-error')).toContainText("Can't count agents right now: The office couldn't read its storage.");
	await expect(page.getByRole('link', { name: /Offices/ }).first()).toBeVisible(); // the window around it is intact
	await expect(rows(page).first().getByTestId('agent-count')).not.toContainText('agents');
	await expect(page.getByTestId('unplaced-note')).toHaveCount(0);
	await rows(page).first().getByRole('button').click();
	await expect(page.getByTestId('office-details')).toContainText("Couldn't be counted");
	await shot(page, 'offices-count-error');
});
