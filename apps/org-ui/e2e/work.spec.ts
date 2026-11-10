import { expect, test, type Page } from '@playwright/test';
import { visit } from './ready';
import { shot } from './shot';

async function open(page: Page) {
	await visit(page, '/create');
	await page.getByLabel('Organization name').fill('Acme');
	await page.getByRole('button', { name: 'Create organization' }).click();
	await expect(page).toHaveURL(/\/agents$/);
}
async function addTask(page: Page, project: string, title: string) {
	await visit(page, `/projects/${project}`);
	await page.getByRole('tab', { name: 'Tasks' }).click();
	await page.getByRole('button', { name: 'New task' }).click();
	const dialog = page.getByRole('dialog', { name: 'New task' });
	await dialog.getByLabel('Title').fill(title);
	await dialog.getByRole('button', { name: 'Create task' }).click();
	await expect(dialog).toBeHidden();
}
async function addBug(page: Page, project: string, title: string, severity: string) {
	await visit(page, `/projects/${project}`);
	await page.getByRole('tab', { name: 'Bugs' }).click();
	await page.getByRole('button', { name: 'Report a bug' }).click();
	const dialog = page.getByRole('dialog', { name: 'Report a bug' });
	await dialog.getByLabel('Title').fill(title);
	await dialog.getByLabel('What happened').fill('It broke');
	await dialog.getByRole('radio', { name: new RegExp(`^${severity}`) }).check();
	await dialog.getByRole('button', { name: 'Report bug' }).click();
	await expect(dialog).toBeHidden();
}
const rows = (page: Page) => page.getByTestId('work-row');

test('WORK: with no work yet the screen says so, and it is in the nav', async ({ page }) => {
	await open(page);
	await page.getByRole('link', { name: 'Work', exact: true }).click();
	await expect(page).toHaveURL(/\/work$/);
	await expect(page.getByTestId('needs-you')).toHaveText('Nothing needs you.');
	await expect(page.getByTestId('no-match')).toBeVisible();
});

test('WORK: tasks and bugs from every project with a Project column, saved filters with counts, and what needs the owner', async ({ page }) => {
	await open(page);
	await addTask(page, 'atlas', 'Atlas task');
	await addTask(page, 'beacon', 'Beacon task');
	await addBug(page, 'atlas', 'Atlas crash', 'Critical');
	await addBug(page, 'beacon', 'Beacon typo', 'Trivial');
	await visit(page, '/work');
	// Tasks open on All (nothing is blocked), grouped by project.
	await expect(rows(page)).toHaveCount(2);
	await expect(page.getByTestId('work-project')).toHaveText(['Atlas', 'Beacon']);
	await expect(page.locator('[data-filter=all]')).toContainText('2');
	// Bugs open on Needs my triage: both are New. Two bugs need the owner.
	await page.getByRole('tab', { name: 'Bugs' }).click();
	await expect(page.locator('[data-filter=triage]')).toHaveAttribute('aria-pressed', 'true');
	await expect(rows(page)).toHaveCount(2);
	await expect(page.getByTestId('needs-you')).toHaveText('2 items need you');
	await page.locator('[data-filter=serious]').click();
	await expect(rows(page)).toHaveCount(1); // Critical and Major bugs: only Atlas crash
	await expect(rows(page)).toContainText('Atlas crash');
	await rows(page).getByRole('button', { name: 'Atlas crash' }).click();
	await page.getByRole('dialog').getByRole('button', { name: 'Triage' }).click();
	await expect(page.getByRole('dialog').getByTestId('bug-status')).toHaveText('Triaged');
	await page.keyboard.press('Escape');
	await expect(page.getByTestId('needs-you')).toHaveText('1 item needs you');
	await shot(page, 'work-bugs');
});
