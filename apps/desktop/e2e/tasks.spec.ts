import { expect, test, type Page } from '@playwright/test';
import { visit } from './ready';
import { shot } from './shot';

async function open(page: Page) {
	await visit(page, '/create');
	await page.getByLabel('Organization name').fill('Acme');
	await page.getByRole('button', { name: 'Create organization' }).click();
	await expect(page).toHaveURL(/\/agents$/);
	await visit(page, '/projects/atlas');
	await page.getByRole('tab', { name: 'Tasks' }).click();
}
const column = (page: Page, status: string) => page.locator(`[data-testid=task-column][data-status=${status}]`);
const card = (page: Page, title: string) => page.getByTestId('task-card').filter({ hasText: title });
async function newTask(page: Page, title: string, priority?: string) {
	await page.getByRole('button', { name: 'New task' }).click();
	const dialog = page.getByRole('dialog', { name: 'New task' });
	await dialog.getByLabel('Title').fill(title);
	if (priority) await dialog.getByLabel('Priority').selectOption(priority);
	await dialog.getByRole('button', { name: 'Create task' }).click();
	await expect(dialog).toBeHidden();
}

test('TSK-1: the board has To do, In progress, Blocked and Done columns', async ({ page }) => {
	await open(page);
	await expect(page.getByTestId('task-column')).toHaveCount(4);
	await expect(page.getByTestId('task-column').locator('h3')).toHaveText([/To do/, /In progress/, /Blocked/, /Done/]);
	await expect(page.getByTestId('no-tasks')).toBeVisible();
});

test('TSK-5 and TSK-10: New task needs only a title, starts in To do with Normal priority (not shown), owner first in the assignee list', async ({ page }) => {
	await open(page);
	await page.getByRole('button', { name: 'New task' }).click();
	const dialog = page.getByRole('dialog', { name: 'New task' });
	await dialog.getByRole('button', { name: 'Create task' }).click();
	await expect(dialog.getByRole('alert')).toHaveText('Give the task a title.');
	const options = await dialog.getByLabel('Assignee').locator('option').allInnerTexts();
	expect(options[0]).toBe('Unassigned');
	expect(options[1]).toBe('Alex Kim (you)');
	await dialog.getByLabel('Title').fill('Draft the launch plan');
	await dialog.getByLabel('Definition of done').fill('A plan the owner approved');
	await dialog.getByRole('button', { name: 'Create task' }).click();
	await expect(page.getByText('Task T-1 created.')).toBeVisible();
	await expect(column(page, 'todo').getByTestId('task-card')).toHaveCount(1);
	await expect(card(page, 'Draft the launch plan')).toContainText('T-1');
	await expect(card(page, 'Draft the launch plan').getByTestId('priority')).toHaveCount(0);
	await newTask(page, 'Fix the build', 'urgent');
	await expect(card(page, 'Fix the build').getByTestId('priority')).toHaveText('Urgent');
	// TSK-4: Urgent first.
	await expect(column(page, 'todo').getByTestId('task-card').first()).toContainText('Fix the build');
	await shot(page, 'tasks-board');
});

test('TSK-2 and TSK-3: a card moves with Move to…, announced; Blocked asks for a reason and the card shows it', async ({ page }) => {
	await open(page);
	await newTask(page, 'Write docs');
	await card(page, 'Write docs').getByLabel(/^Move T-1 to/).selectOption('in_progress');
	await expect(column(page, 'in_progress').getByTestId('task-card')).toHaveCount(1);
	await expect(page.getByRole('status').filter({ hasText: 'T-1 moved to In progress.' })).toHaveCount(1);
	await card(page, 'Write docs').getByLabel(/^Move T-1 to/).selectOption('blocked');
	const dialog = page.getByRole('dialog', { name: 'Block T-1' });
	await expect(dialog.getByRole('button', { name: 'Block task' })).toBeDisabled();
	await dialog.getByLabel('Blocked on').fill('Waiting on design');
	await dialog.getByRole('button', { name: 'Block task' }).click();
	await expect(column(page, 'blocked').getByTestId('card-blocked')).toHaveText('Blocked: Waiting on design');
	await shot(page, 'tasks-blocked');
});

test('CHK: the task page assigns, keeps a checklist and moves the task to Done', async ({ page }) => {
	await open(page);
	await newTask(page, 'Ship it');
	await card(page, 'Ship it').getByRole('button', { name: /Ship it/ }).click();
	const sheet = page.getByRole('dialog');
	await expect(sheet.getByTestId('task-time')).toContainText('so far');
	await sheet.getByLabel('Assignee').selectOption({ label: 'Alex Kim (you)' });
	await sheet.getByLabel('New step').fill('Write tests');
	await sheet.getByRole('button', { name: 'Add step' }).click();
	await expect(sheet.getByTestId('checklist')).toContainText('Write tests');
	await sheet.getByRole('checkbox', { name: 'Write tests' }).click();
	await expect(sheet.getByRole('heading', { name: /Checklist/ })).toContainText('1 of 1');
	await sheet.getByLabel('Status').selectOption('done');
	await expect(sheet.getByLabel('Status')).toHaveValue('done');
	await shot(page, 'task-page');
	await page.keyboard.press('Escape');
	await expect(column(page, 'done').getByTestId('task-card')).toHaveCount(1);
	await expect(card(page, 'Ship it')).toContainText('Alex Kim');
	await expect(card(page, 'Ship it')).toContainText('1 of 1 step');
});

test('CHK: Done is guarded by a required step; done anyway records why, and reopening clears it', async ({ page }) => {
	await open(page);
	await newTask(page, 'Careful work');
	await card(page, 'Careful work').getByRole('button', { name: /Careful work/ }).click();
	const sheet = page.getByRole('dialog');
	await sheet.getByLabel('New step').fill('Review the diff');
	await sheet.getByRole('checkbox', { name: 'Required' }).click();
	await sheet.getByRole('button', { name: 'Add step' }).click();
	await expect(sheet.getByTestId('checklist')).toContainText('Required');
	await sheet.getByLabel('Status').selectOption('done');
	await expect(sheet.getByRole('alert')).toContainText('Required steps are not done: Review the diff');
	await expect(sheet.getByLabel('Status')).toHaveValue('todo');
	await sheet.getByLabel('Why is it done with required steps left?').fill('Reviewed in the pairing session');
	await sheet.getByRole('button', { name: 'Mark done anyway' }).click();
	await expect(sheet.getByTestId('done-anyway')).toHaveText('Done anyway: Reviewed in the pairing session');
	await sheet.getByLabel('Status').selectOption('todo');
	await expect(sheet.getByTestId('done-anyway')).toHaveCount(0);
});

test('CHK-5: moving a card to Done with required steps left opens the task with what to do', async ({ page }) => {
	await open(page);
	await newTask(page, 'Guarded');
	await card(page, 'Guarded').getByRole('button', { name: /Guarded/ }).click();
	let sheet = page.getByRole('dialog');
	await sheet.getByLabel('New step').fill('Sign off');
	await sheet.getByRole('checkbox', { name: 'Required' }).click();
	await sheet.getByRole('button', { name: 'Add step' }).click();
	await expect(sheet.getByTestId('checklist')).toContainText('Sign off');
	await page.keyboard.press('Escape');
	await card(page, 'Guarded').getByLabel(/^Move T-1 to/).selectOption('done');
	sheet = page.getByRole('dialog');
	await expect(sheet.getByRole('alert')).toContainText('Required steps are not done: Sign off');
	await expect(sheet.getByLabel('Why is it done with required steps left?')).toBeVisible();
});

test('the task page keeps a typed description when another field changes, and the Status picker follows a cancelled Block', async ({ page }) => {
	await open(page);
	await newTask(page, 'Keep my words');
	await card(page, 'Keep my words').getByRole('button', { name: /Keep my words/ }).click();
	const sheet = page.getByRole('dialog');
	await sheet.getByLabel('Description').fill('A long explanation');
	await sheet.getByLabel('Priority').selectOption('high');
	await expect(sheet.getByLabel('Priority')).toHaveValue('high');
	await expect(sheet.getByLabel('Description')).toHaveValue('A long explanation');
	await sheet.getByLabel('Status').selectOption('blocked');
	await sheet.getByRole('button', { name: 'Cancel' }).click();
	await expect(sheet.getByLabel('Status')).toHaveValue('todo');
	await expect(page.getByRole('status').filter({ hasText: 'priority is now High.' })).toHaveCount(1);
});

test('TSK-9: Board or List is remembered per project', async ({ page }) => {
	await open(page);
	await newTask(page, 'Listed task', 'high');
	await page.getByRole('button', { name: 'List', exact: true }).click();
	await expect(page.getByTestId('task-list')).toBeVisible();
	await expect(page.getByTestId('task-row')).toHaveCount(1);
	await expect(page.getByTestId('task-row')).toContainText('T-1');
	await expect(page.getByTestId('task-row')).toContainText('High');
	await shot(page, 'tasks-list');
	await visit(page, '/projects/atlas');
	await page.getByRole('tab', { name: 'Tasks' }).click();
	await expect(page.getByTestId('task-list')).toBeVisible(); // remembered
	await page.getByRole('button', { name: 'Board', exact: true }).click();
	await expect(page.getByTestId('task-board')).toBeVisible();
});

test('the definition of done is kept with the task and can be changed on the task page', async ({ page }) => {
	await open(page);
	await newTask(page, 'Goal task');
	await card(page, 'Goal task').getByRole('button', { name: /Goal task/ }).click();
	const sheet = page.getByRole('dialog');
	await sheet.getByLabel('Definition of done').fill('Every test passes');
	await sheet.getByLabel('Definition of done').blur(); // one save model: a field saves when it is left
	await expect(sheet.getByTestId('saved')).toHaveText('Definition of done saved.');
	await sheet.getByLabel('Status').selectOption('done');
	await expect(sheet.getByLabel('Status').locator('option').first()).toHaveText('Done · Completed');
});

test('TSK-5: assigning to a paused agent asks first, on the task page and in New task; the picker shows runtime and status', async ({ page }) => {
	await open(page);
	// Pause an agent first (the Agents screen is the place for it).
	await visit(page, '/org-chart');
	await page.locator('[data-testid=team-card][data-team=web]').getByRole('button', { name: /^Noor Haddad/ }).click();
	await page.getByTestId('selected-member').getByRole('button', { name: 'Pause…' }).click();
	await page.getByRole('dialog').getByRole('button', { name: 'Pause Noor Haddad' }).click();
	await expect(page.getByText('Paused Noor Haddad.')).toBeVisible();
	await visit(page, '/projects/atlas');
	await page.getByRole('tab', { name: 'Tasks' }).click();
	await page.getByRole('button', { name: 'New task' }).click();
	const dialog = page.getByRole('dialog', { name: 'New task' });
	const options = await dialog.getByLabel('Assignee').locator('option').allInnerTexts();
	expect(options).toContain('Noor Haddad (agent · Claude Code · Paused)');
	expect(options.some((o) => /\(agent · .* · Asleep\)$/.test(o))).toBe(true);
	await dialog.getByLabel('Title').fill('For Noor');
	await dialog.getByLabel('Assignee').selectOption({ label: 'Noor Haddad (agent · Claude Code · Paused)' });
	await dialog.getByRole('button', { name: 'Create task' }).click();
	await expect(dialog.getByTestId('assign-warning')).toContainText('Noor Haddad is paused. The task waits until you resume them. Create it anyway?');
	await dialog.getByRole('button', { name: 'Create anyway' }).click();
	await expect(dialog).toBeHidden();
	await expect(page.getByTestId('task-card').filter({ hasText: 'For Noor' })).toContainText('Noor Haddad');
	await newTask(page, 'Unassigned one');
	await page.getByTestId('task-card').filter({ hasText: 'Unassigned one' }).getByRole('button', { name: /Unassigned one/ }).click();
	const sheet = page.getByRole('dialog');
	await sheet.getByLabel('Assignee').selectOption({ label: 'Noor Haddad (agent · Claude Code · Paused)' });
	await expect(sheet.getByTestId('assign-warning')).toBeVisible();
	await sheet.getByRole('button', { name: 'Cancel' }).click();
	await expect(sheet.getByLabel('Assignee')).toHaveValue('');
	await sheet.getByLabel('Assignee').selectOption({ label: 'Noor Haddad (agent · Claude Code · Paused)' });
	await sheet.getByRole('button', { name: 'Assign anyway' }).click();
	await expect(sheet.getByLabel('Assignee')).toHaveValue('noor');
});
