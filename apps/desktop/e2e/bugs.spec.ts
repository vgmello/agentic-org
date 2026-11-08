import { expect, test, type Page } from '@playwright/test';
import { visit } from './ready';
import { shot } from './shot';

async function open(page: Page) {
	await visit(page, '/create');
	await page.getByLabel('Organization name').fill('Acme');
	await page.getByRole('button', { name: 'Create organization' }).click();
	await expect(page).toHaveURL(/\/agents$/);
	await visit(page, '/projects/atlas');
	await page.getByRole('tab', { name: 'Bugs' }).click();
}
const row = (page: Page, title: string) => page.getByTestId('bug-row').filter({ hasText: title });
async function report(page: Page, title: string, severity = 'Major') {
	await page.getByRole('button', { name: 'Report a bug' }).click();
	const dialog = page.getByRole('dialog', { name: 'Report a bug' });
	await dialog.getByLabel('Title').fill(title);
	await dialog.getByLabel('What happened').fill('It broke when I clicked save');
	await dialog.getByRole('radio', { name: new RegExp(`^${severity}`) }).check();
	await dialog.getByRole('button', { name: 'Report bug' }).click();
	await expect(dialog).toBeHidden();
}

test('BUG-1: Report a bug needs a title, what happened and a severity (no default), and lands in New', async ({ page }) => {
	await open(page);
	await expect(page.getByTestId('no-bugs')).toBeVisible();
	await page.getByRole('button', { name: 'Report a bug' }).click();
	const dialog = page.getByRole('dialog', { name: 'Report a bug' });
	await expect(dialog.getByRole('radio', { checked: true })).toHaveCount(0);
	await dialog.getByRole('button', { name: 'Report bug' }).click();
	await expect(dialog.getByRole('alert')).toHaveText(['Give the bug a title.', 'Say what happened.', 'Choose a severity.']);
	await shot(page, 'report-bug');
	await dialog.getByLabel('Title').fill('Save button does nothing');
	await dialog.getByLabel('What happened').fill('Nothing at all');
	await dialog.getByRole('radio', { name: /^Critical/ }).check();
	await dialog.getByRole('button', { name: 'Report bug' }).click();
	await expect(page.getByText('Bug B-1 reported.')).toBeVisible();
	await expect(row(page, 'Save button does nothing')).toContainText('B-1');
	await expect(row(page, 'Save button does nothing')).toContainText('Critical?'); // not confirmed yet
	await expect(row(page, 'Save button does nothing')).toContainText('New');
});

test('BUG-4: similar open bugs are suggested while the title is typed', async ({ page }) => {
	await open(page);
	await report(page, 'Upload fails on large files');
	await page.getByRole('button', { name: 'Report a bug' }).click();
	const dialog = page.getByRole('dialog', { name: 'Report a bug' });
	await dialog.getByLabel('Title').fill('Upload fails again');
	await expect(dialog.getByTestId('similar-bugs')).toContainText('B-1 · Upload fails on large files');
});

test('TRI and FIX: triage a New bug, mark it fixed, then verify it, and a closed bug can be reopened', async ({ page }) => {
	await open(page);
	await report(page, 'Wrong total');
	await row(page, 'Wrong total').getByRole('button', { name: 'Wrong total' }).click();
	const sheet = page.getByRole('dialog');
	await expect(sheet.getByTestId('triage-panel')).toContainText('Suggested triager: Alex Kim');
	await sheet.getByLabel('Severity').selectOption('minor');
	await sheet.getByLabel('Priority').selectOption('high');
	await sheet.getByLabel('Assignee').selectOption({ label: 'Alex Kim (you)' });
	await sheet.getByRole('button', { name: 'Triage' }).click();
	await expect(sheet.getByTestId('bug-status')).toHaveText('Triaged');
	await expect(sheet.getByTestId('bug-severity')).toHaveText('Minor');
	await sheet.getByRole('button', { name: 'Mark fixed…' }).click();
	await expect(sheet.getByRole('button', { name: 'Mark fixed' })).toBeDisabled();
	await sheet.getByLabel('What fixed it').fill('Rounded the total');
	await sheet.getByRole('button', { name: 'Mark fixed' }).click();
	await expect(sheet.getByTestId('bug-status')).toHaveText('Fixed');
	await expect(sheet.getByTestId('bug-reason')).toHaveText('Fix: Rounded the total');
	await shot(page, 'bug-page');
	await sheet.getByRole('button', { name: 'Still happens…' }).click();
	await sheet.getByLabel('What do you still see').fill('Still off by a cent');
	await sheet.getByRole('button', { name: 'Send back' }).click();
	await expect(sheet.getByTestId('bug-status')).toHaveText('Triaged');
	await expect(sheet).toContainText('Reopened 1 time');
	await sheet.getByRole('button', { name: 'Mark fixed…' }).click();
	await sheet.getByLabel('What fixed it').fill('Used decimals');
	await sheet.getByRole('button', { name: 'Mark fixed' }).click();
	await sheet.getByRole('button', { name: "It's fixed: close" }).click();
	await expect(sheet.getByTestId('bug-status')).toHaveText('Closed · Verified');
	await sheet.getByRole('button', { name: 'Reopen…' }).click();
	await sheet.getByLabel('Why is it reopened').fill('Back in v2');
	await sheet.getByRole('button', { name: 'Reopen', exact: true }).click();
	await expect(sheet.getByTestId('bug-status')).toHaveText('Triaged');
});

test('BUG: closing as a duplicate names the original; the filters and counts follow', async ({ page }) => {
	await open(page);
	await report(page, 'Crash on start');
	await report(page, 'App crashes at launch');
	await row(page, 'App crashes at launch').getByRole('button', { name: /App crashes/ }).click();
	const sheet = page.getByRole('dialog');
	await sheet.getByRole('button', { name: 'Close…' }).click();
	await sheet.getByLabel('Close it as').selectOption('duplicate');
	await expect(sheet.getByRole('button', { name: 'Close bug' })).toBeDisabled();
	await sheet.getByLabel('The original').selectOption({ label: 'B-1 · Crash on start' });
	await sheet.getByRole('button', { name: 'Close bug' }).click();
	await expect(sheet.getByTestId('bug-status')).toHaveText('Closed · Duplicate of B-1');
	await page.keyboard.press('Escape');
	await expect(page.getByRole('button', { name: /^Open/ })).toContainText('1');
	await page.getByRole('button', { name: /^Closed/ }).click();
	await expect(page.getByTestId('bug-row')).toHaveCount(1);
	await expect(page.getByTestId('bug-row')).toContainText('Duplicate of B-1');
});

test('FIX: a fix task drives its bug: In progress when it starts, Fixed when it is Done, and the bug page lists it', async ({ page }) => {
	await open(page);
	await report(page, 'Total is wrong');
	await row(page, 'Total is wrong').getByRole('button', { name: 'Total is wrong' }).click();
	let sheet = page.getByRole('dialog');
	await sheet.getByRole('button', { name: 'Triage' }).click();
	await expect(sheet.getByTestId('bug-status')).toHaveText('Triaged');
	await expect(sheet.getByTestId('fix-tasks')).toContainText('No fix task yet.');
	await sheet.getByRole('button', { name: 'Create a fix task' }).click();
	await expect(sheet.getByTestId('fix-task')).toContainText('T-1');
	await expect(sheet.getByTestId('fix-task')).toContainText('To do');
	await page.keyboard.press('Escape');

	// The task starts: the bug follows.
	await page.getByRole('tab', { name: 'Tasks' }).click();
	const task = page.getByTestId('task-card').filter({ hasText: 'Fix B-1' });
	await task.getByRole('button', { name: /^Actions for/ }).click();
	await page.getByRole('menuitem', { name: 'In progress', exact: true }).click();
	await page.getByRole('tab', { name: 'Bugs' }).click();
	await expect(row(page, 'Total is wrong')).toContainText('In progress');

	// The task is done: the bug is Fixed, waiting to be verified.
	await page.getByRole('tab', { name: 'Tasks' }).click();
	await page.getByTestId('task-card').filter({ hasText: 'Fix B-1' }).getByRole('button', { name: /^Actions for/ }).click();
	await page.getByRole('menuitem', { name: 'Done', exact: true }).click();
	await page.getByRole('tab', { name: 'Bugs' }).click();
	await expect(row(page, 'Total is wrong')).toContainText('Fixed');
	await row(page, 'Total is wrong').getByRole('button', { name: 'Total is wrong' }).click();
	sheet = page.getByRole('dialog');
	await expect(sheet.getByTestId('fix-task')).toContainText('Done');
	await shot(page, 'bug-fix-task');
	await page.keyboard.press('Escape');

	// The task page says what it fixes.
	await page.getByRole('tab', { name: 'Tasks' }).click();
	await page.getByRole('button', { name: 'Show all done' }).click();
	await page.getByTestId('task-card').filter({ hasText: 'Fix B-1' }).getByRole('button', { name: /Fix B-1/ }).click();
	await expect(page.getByTestId('task-fixes')).toContainText('Fixes B-1 · Total is wrong');
});
