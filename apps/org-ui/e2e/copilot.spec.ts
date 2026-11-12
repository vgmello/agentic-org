import { expect, test, type Page } from '@playwright/test';
import { visit } from './ready';
import { shot } from './shot';

// The smart features on the TypeScript mock, in the real screens: "Report a bug from a sentence" in
// the Report a bug dialog, and "Draft or improve" on the bug and task text fields.
// `?copilot=off|failed|timeout` picks a scenario (src/lib/modules/copilot/mock.ts).

async function openProject(page: Page, tab: 'Bugs' | 'Tasks', scenario = '') {
	await visit(page, '/create');
	await page.getByLabel('Organization name').fill('Acme');
	await page.getByRole('button', { name: 'Create organization' }).click();
	await expect(page).toHaveURL(/\/agents$/);
	await visit(page, `/projects/atlas${scenario ? `?copilot=${scenario}` : ''}`);
	await page.getByRole('tab', { name: tab }).click();
}

async function openReportDialog(page: Page, scenario = '') {
	await openProject(page, 'Bugs', scenario);
	await page.getByRole('button', { name: 'Report a bug' }).click();
	const dialog = page.getByRole('dialog', { name: 'Report a bug' });
	await expect(dialog).toBeVisible();
	return dialog;
}

const smartButtons = (scope: ReturnType<Page['locator']>) => scope.getByRole('button', { name: /^Draft or improve/ });
const bugRows = (page: Page) => page.getByTestId('bug-row');

test('report dialog: a sentence drafts the fields of the real form, and nothing is reported until the person confirms', { tag: '@journey' }, async ({ page }) => {
	const dialog = await openReportDialog(page);
	await dialog.getByLabel('What went wrong?').fill("The save button doesn't work after I rename a file. It just sits there.");
	await dialog.getByRole('button', { name: 'Draft the report' }).click();

	await expect(dialog.getByTestId('drafted-banner')).toContainText('Drafted by the copilot. Nothing is reported yet.');
	await expect(dialog.getByLabel('Title')).toHaveValue("The save button doesn't work after I rename a file");
	await expect(dialog.getByLabel('What happened')).toHaveValue(/sits there/);
	// BUG-1: a bug has no default severity. The copilot's pick (the office's own scale) is a suggestion
	// beside the options, never a selected one.
	await expect(dialog.getByRole('radio', { checked: true })).toHaveCount(0);
	await expect(dialog.getByTestId('severity-suggestion')).toHaveText('The copilot suggests Major. You choose.');
	await expect(dialog.getByTestId('severity-suggested')).toHaveText('Suggested by the copilot');
	// The drafted fields are tagged, so checking doesn't mean re-reading everything.
	await expect(dialog.getByTestId('drafted-tag-bug-title')).toBeVisible();
	await expect(dialog.getByTestId('drafted-tag-bug-happened')).toBeVisible();
	await expect(dialog.getByTestId('drafted-tag-bug-steps')).toHaveCount(0);
	await expect(dialog.getByLabel('Area')).toHaveValue('atlas-general');
	// The app's own environment line is not overwritten by a draft that doesn't have one.
	await expect(dialog.getByLabel('Environment')).toHaveValue(/^This Mac \(HQ\)/);
	await expect(bugRows(page)).toHaveCount(0);
	await shot(page, 'report-dialog-drafted');

	// The person edits the title: its tag goes, the other field keeps its tag.
	await dialog.getByLabel('Title').fill('Save does nothing after a rename');
	await expect(dialog.getByTestId('drafted-tag-bug-title')).toHaveCount(0);
	await expect(dialog.getByTestId('drafted-tag-bug-happened')).toBeVisible();
	// Typing it back to the draft's words doesn't bring the tag back: it is the person's text now.
	await dialog.getByLabel('Title').fill("The save button doesn't work after I rename a file");
	await expect(dialog.getByTestId('drafted-tag-bug-title')).toHaveCount(0);
	await dialog.getByLabel('Title').fill('Save does nothing after a rename');

	// Report bug still needs a severity the person picked.
	await dialog.getByRole('button', { name: 'Report bug' }).click();
	await expect(dialog.getByRole('alert')).toHaveText('Choose a severity.');
	await expect(bugRows(page)).toHaveCount(0);
	await dialog.getByRole('radio', { name: /^Major/ }).check();
	await expect(dialog.getByTestId('severity-suggestion')).toHaveCount(0);
	await expect(dialog.getByTestId('severity-suggested')).toHaveCount(0);
	await dialog.getByRole('button', { name: 'Report bug' }).click();
	await expect(page.getByText('Bug B-1 reported.')).toBeVisible();
	await expect(bugRows(page)).toHaveCount(1);
	await expect(bugRows(page).first()).toContainText('Save does nothing after a rename');
	await expect(bugRows(page).first()).toContainText('Major?');
});

test('report dialog: Undo draft brings back the person’s own text, not an earlier draft', async ({ page }) => {
	const dialog = await openReportDialog(page);
	await dialog.getByLabel('Title').fill('My own title');
	await dialog.getByLabel('What happened').fill('My own words');
	await dialog.getByLabel('What went wrong?').fill('It crashed on start');
	await dialog.getByRole('button', { name: 'Draft the report' }).click();
	await expect(dialog.getByTestId('severity-suggestion')).toHaveText('The copilot suggests Critical. You choose.');
	await expect(dialog.getByRole('radio', { checked: true })).toHaveCount(0);
	await expect(dialog.getByLabel('Title')).toHaveValue('It crashed on start');

	// A second draft replaces the first.
	await dialog.getByLabel('What went wrong?').fill('A typo in the header');
	await dialog.getByRole('button', { name: 'Draft the report' }).click();
	await expect(dialog.getByTestId('severity-suggestion')).toHaveText('The copilot suggests Trivial. You choose.');
	await expect(dialog.getByLabel('Title')).toHaveValue('A typo in the header');
	await expect(dialog.getByTestId('drafted-tag-bug-title')).toBeVisible();

	await dialog.getByRole('button', { name: 'Undo draft' }).click();
	await expect(dialog.getByLabel('Title')).toHaveValue('My own title');
	await expect(dialog.getByLabel('What happened')).toHaveValue('My own words');
	await expect(dialog.getByRole('radio', { checked: true })).toHaveCount(0);
	await expect(dialog.getByTestId('drafted-banner')).toHaveCount(0);
	// The person's own text is not "Drafted", and the suggestion goes with the draft.
	await expect(dialog.getByTestId('drafted-tag-bug-title')).toHaveCount(0);
	await expect(dialog.getByTestId('severity-suggestion')).toHaveCount(0);
});

test('report dialog: a severity the person chose is kept, and a later draft only suggests', async ({ page }) => {
	const dialog = await openReportDialog(page);
	await dialog.getByRole('radio', { name: /^Minor/ }).check();
	await dialog.getByLabel('What went wrong?').fill('It crashed on start');
	await dialog.getByRole('button', { name: 'Draft the report' }).click();
	await expect(dialog.getByLabel('Title')).toHaveValue('It crashed on start');
	await expect(dialog.getByRole('radio', { name: /^Minor/ })).toBeChecked();
	// Nothing to suggest while the person has already chosen.
	await expect(dialog.getByTestId('severity-suggestion')).toHaveCount(0);
});

test('report dialog: Undo cancels a draft that is still on its way', async ({ page }) => {
	const dialog = await openReportDialog(page);
	await dialog.getByLabel('Title').fill('My own title');
	await dialog.getByLabel('What went wrong?').fill('It crashed on start');
	await dialog.getByRole('button', { name: 'Draft the report' }).click();
	await expect(dialog.getByText('Drafting the report…')).toBeVisible();
	await dialog.getByRole('button', { name: 'Cancel draft' }).click();
	// Longer than the mock's draft takes: the cancelled draft must not land on the form.
	await page.waitForTimeout(1200);
	await expect(dialog.getByLabel('Title')).toHaveValue('My own title');
	await expect(dialog.getByTestId('drafted-banner')).toHaveCount(0);
	await expect(dialog.getByRole('radio', { checked: true })).toHaveCount(0);

	// And a second draft cancelled after a first one was applied puts the person's text back.
	await dialog.getByRole('button', { name: 'Draft the report' }).click();
	await expect(dialog.getByLabel('Title')).toHaveValue('It crashed on start');
	await dialog.getByLabel('What went wrong?').fill('Search fails');
	await dialog.getByRole('button', { name: 'Draft the report' }).click();
	await dialog.getByRole('button', { name: 'Undo draft' }).click();
	await page.waitForTimeout(1200);
	await expect(dialog.getByLabel('Title')).toHaveValue('My own title');
});

test('report dialog: reporting while a draft is on its way drops the draft', async ({ page }) => {
	const dialog = await openReportDialog(page);
	await dialog.getByLabel('Title').fill('Typed title');
	await dialog.getByLabel('What happened').fill('Typed text');
	await dialog.getByRole('radio', { name: /^Minor/ }).check();
	await dialog.getByLabel('What went wrong?').fill('It crashed on start');
	await dialog.getByRole('button', { name: 'Draft the report' }).click();
	await dialog.getByRole('button', { name: 'Report bug' }).click();
	await expect(page.getByText('Bug B-1 reported.')).toBeVisible();
	await expect(bugRows(page).first()).toContainText('Typed title');
	await page.waitForTimeout(1000);
	// A fresh dialog is empty: the late draft did not land on it.
	await page.getByRole('button', { name: 'Report a bug' }).click();
	await expect(dialog.getByLabel('Title')).toHaveValue('');
});

test('report dialog: Draft or improve is on the title, what happened, expected and steps, and replaces the text only when the person says so', async ({ page }) => {
	const dialog = await openReportDialog(page);
	await expect(smartButtons(dialog)).toHaveCount(4);
	const field = dialog.getByLabel('What happened');
	await field.fill('  the save  button does nothing');

	await dialog.getByRole('button', { name: 'Draft or improve What happened' }).click();
	const panel = dialog.getByTestId('copilot-bug-happened');
	await expect(panel).toContainText('Improve this text');
	await expect(panel.getByTestId('copilot-sample')).toHaveText('Sample data');
	await panel.getByRole('button', { name: 'Improve' }).click();
	await expect(panel.getByTestId('copilot-proposal')).toHaveText('The save button does nothing.');
	// It is only a proposal: the field still holds what the person wrote.
	await expect(field).toHaveValue('  the save  button does nothing');
	await shot(page, 'report-dialog-improve-proposal', { context: panel });

	await panel.getByRole('button', { name: 'Use this draft' }).click();
	await expect(field).toHaveValue('The save button does nothing.');
	await expect(panel).toHaveCount(0);
	// The field says it holds a draft, until the person edits it or takes it back.
	await expect(dialog.getByTestId('drafted-tag-bug-happened')).toBeVisible();
	await field.fill('The save button does nothing. I edited it.');
	await expect(dialog.getByTestId('drafted-tag-bug-happened')).toHaveCount(0);
	await dialog.getByRole('button', { name: 'Undo', exact: true }).click();
	await expect(field).toHaveValue('  the save  button does nothing');
	await expect(dialog.getByTestId('drafted-tag-bug-happened')).toHaveCount(0);
});

test('report dialog field: Undo after two drafts brings back the person’s own text, not the first draft', async ({ page }) => {
	const dialog = await openReportDialog(page);
	const field = dialog.getByLabel('What happened');
	await field.fill('foo');
	for (const instruction of ['make it shorter', 'make it friendlier']) {
		await dialog.getByRole('button', { name: 'Draft or improve What happened' }).click();
		const panel = dialog.getByTestId('copilot-bug-happened');
		await panel.getByLabel('Instruction (optional)').fill(instruction);
		await panel.getByRole('button', { name: 'Improve' }).click();
		await panel.getByRole('button', { name: 'Use this draft' }).click();
	}
	await expect(field).toHaveValue('Foo.');
	await field.fill('Foo. And my own edit');
	// The person edited the draft: that edit is their text now, and the next draft goes past it.
	await dialog.getByRole('button', { name: 'Draft or improve What happened' }).click();
	await dialog.getByTestId('copilot-bug-happened').getByRole('button', { name: 'Improve' }).click();
	await dialog.getByTestId('copilot-bug-happened').getByRole('button', { name: 'Use this draft' }).click();
	await dialog.getByRole('button', { name: 'Undo', exact: true }).click();
	await expect(field).toHaveValue('Foo. And my own edit');
});

test('report dialog: the fields are locked while a draft is on its way, so it cannot overwrite typing', async ({ page }) => {
	const dialog = await openReportDialog(page);
	await dialog.getByLabel('What went wrong?').fill('It crashed on start');
	await dialog.getByRole('button', { name: 'Draft the report' }).click();
	await expect(dialog.getByLabel('Title')).toBeDisabled();
	await expect(dialog.getByLabel('What happened')).toBeDisabled();
	// Capture while the spinner is intentionally running, before the draft unlocks the fields.
	await shot(page, 'report-dialog-drafting', { settle: false });
	await expect(dialog.getByLabel('Title')).toBeEnabled();
	await expect(dialog.getByLabel('Title')).toHaveValue('It crashed on start');
});

test('report dialog: Enter in the instruction field asks the copilot and does not submit the report', async ({ page }) => {
	const dialog = await openReportDialog(page);
	await dialog.getByLabel('Title').fill('Typed title');
	await dialog.getByRole('button', { name: 'Draft or improve Steps to reproduce' }).click();
	const panel = dialog.getByTestId('copilot-bug-steps');
	await panel.getByLabel('Instruction (optional)').fill('open a file then press save');
	await panel.getByLabel('Instruction (optional)').press('Enter');
	await expect(panel.getByTestId('copilot-proposal')).toContainText('Open a file then press save');
	await expect(dialog).toBeVisible();
	await expect(bugRows(page)).toHaveCount(0);
});

test('report dialog: a draft longer than the field allows is cut to fit, and the person is told', async ({ page }) => {
	const dialog = await openReportDialog(page);
	// The title allows 120 characters (the office's cap); the mock's draft of a long instruction is longer.
	await dialog.getByRole('button', { name: 'Draft or improve Title' }).click();
	const panel = dialog.getByTestId('copilot-bug-title');
	await panel.getByLabel('Instruction (optional)').fill('a'.repeat(200));
	await panel.getByRole('button', { name: 'Draft', exact: true }).click();
	await expect(panel.getByTestId('copilot-proposal')).toBeVisible();
	await panel.getByRole('button', { name: 'Use this draft' }).click();
	const value = await dialog.getByLabel('Title').inputValue();
	expect(value.length).toBeLessThanOrEqual(120);
	expect(value.length).toBeGreaterThan(100);
	await expect(dialog.getByTestId('draft-used')).toContainText('Draft used. Shortened to fit the limit of 120 characters.');
	await shot(page, 'copilot-bug10-clipped-spacing', { context: dialog });
});

test('report dialog: with nothing to go on, Draft asks for an instruction instead of running', async ({ page }) => {
	const dialog = await openReportDialog(page);
	await dialog.getByRole('button', { name: 'Draft or improve Expected' }).click();
	const panel = dialog.getByTestId('copilot-bug-expected');
	await panel.getByRole('button', { name: 'Draft', exact: true }).click();
	await expect(panel.getByRole('alert')).toHaveText('Say what it should be about, then ask for a draft.');
	await expect(panel.getByTestId('copilot-proposal')).toHaveCount(0);
	await shot(page, 'copilot-empty-instruction', { context: panel });
	await panel.getByLabel('Instruction (optional)').fill('the file is saved');
	await expect(panel.getByRole('alert')).toHaveCount(0);
});

test('report dialog: without a runtime there are no smart buttons and the form still reports a bug', async ({ page }) => {
	const dialog = await openReportDialog(page, 'off');
	await expect(dialog.getByLabel('Title')).toBeVisible();
	await expect(smartButtons(dialog)).toHaveCount(0);
	await expect(dialog.getByRole('button', { name: 'Draft the report' })).toHaveCount(0);
	await expect(dialog.getByLabel('What went wrong?')).toHaveCount(0);
	await dialog.getByLabel('Title').fill('Typed by hand');
	await dialog.getByLabel('What happened').fill('It breaks');
	await dialog.getByRole('radio', { name: /^Major/ }).check();
	await dialog.getByRole('button', { name: 'Report bug' }).click();
	await expect(page.getByText('Bug B-1 reported.')).toBeVisible();
	await shot(page, 'report-dialog-no-copilot');
});

for (const [scenario, title] of [
	['failed', "The copilot couldn't draft this"],
	['timeout', 'The copilot took too long']
] as const) {
	test(`report dialog: when the copilot ${scenario}, the person is told and nothing changes`, async ({ page }) => {
		const dialog = await openReportDialog(page, scenario);
		await dialog.getByLabel('What went wrong?').fill('It crashed');
		await dialog.getByRole('button', { name: 'Draft the report' }).click();
		await expect(dialog.getByRole('alert').filter({ hasText: title })).toBeVisible();
		await expect(dialog.getByLabel('Title')).toHaveValue('');
		await expect(dialog.getByTestId('drafted-banner')).toHaveCount(0);

		await dialog.getByLabel('What happened').fill('Some text');
		await dialog.getByRole('button', { name: 'Draft or improve What happened' }).click();
		const panel = dialog.getByTestId('copilot-bug-happened');
		await panel.getByRole('button', { name: 'Improve' }).click();
		await expect(panel.getByRole('alert').filter({ hasText: title })).toBeVisible();
		await expect(dialog.getByLabel('What happened')).toHaveValue('Some text');
		// One error at a time: running the field's copilot cleared the sentence draft's error.
		await expect(dialog.getByRole('alert')).toHaveCount(1);
		await shot(page, `copilot-${scenario}`, { context: panel });

		// Running it again clears the panel's own error while it works, and Dismiss clears it too.
		await panel.getByRole('button', { name: 'Improve' }).click();
		await expect(panel.getByText('Drafting…')).toBeVisible();
		await expect(panel.getByRole('alert')).toHaveCount(0);
		await expect(panel.getByRole('alert').filter({ hasText: title })).toBeVisible();
		await panel.getByRole('button', { name: 'Cancel' }).click();
		await expect(dialog.getByRole('alert')).toHaveCount(0);

		// The sentence draft's error has a Dismiss of its own, and a new draft closes an open panel.
		await dialog.getByRole('button', { name: 'Draft the report' }).click();
		await expect(dialog.getByRole('alert').filter({ hasText: title })).toBeVisible();
		await dialog.getByRole('button', { name: 'Dismiss the draft error' }).click();
		await expect(dialog.getByRole('alert')).toHaveCount(0);
		await expect(dialog.getByRole('button', { name: 'Draft the report' })).toBeFocused();
		await dialog.getByRole('button', { name: 'Draft or improve What happened' }).click();
		await panel.getByRole('button', { name: 'Improve' }).click();
		await expect(panel.getByRole('alert').filter({ hasText: title })).toBeVisible();
		await dialog.getByRole('button', { name: 'Draft the report' }).click();
		await expect(panel).toHaveCount(0);
		await expect(dialog.getByRole('alert').filter({ hasText: title })).toHaveCount(1);
	});
}

test('task dialog: Draft or improve on the description of a new task, and the task is created with the text the person kept', async ({ page }) => {
	await openProject(page, 'Tasks');
	await page.getByRole('button', { name: 'New task' }).click();
	const dialog = page.getByRole('dialog', { name: 'New task' });
	await dialog.getByLabel('Title').fill('Plan the launch');
	await dialog.getByLabel('Description').fill('  write  the launch plan');
	await dialog.getByRole('button', { name: 'Draft or improve Description' }).click();
	const panel = dialog.getByTestId('copilot-new-task-description');
	await panel.getByRole('button', { name: 'Improve' }).click();
	await expect(panel.getByTestId('copilot-proposal')).toHaveText('Write the launch plan.');
	await panel.getByRole('button', { name: 'Use this draft' }).click();
	await expect(dialog.getByLabel('Description')).toHaveValue('Write the launch plan.');
	await dialog.getByRole('button', { name: 'Create task' }).click();
	await expect(page.getByText('Task T-1 created.')).toBeVisible();

	// The saved description is the one the person kept.
	await page.getByTestId('task-card').filter({ hasText: 'Plan the launch' }).getByRole('button', { name: 'Plan the launch' }).click();
	const sheet = page.getByRole('dialog');
	await expect(sheet.getByLabel('Description')).toHaveValue('Write the launch plan.');
	await shot(page, 'task-description-drafted');
});

test('task sheet: a draft used on the description of an existing task is saved like any edit, and Undo is saved too', async ({ page }) => {
	await openProject(page, 'Tasks');
	await page.getByRole('button', { name: 'New task' }).click();
	const dialog = page.getByRole('dialog', { name: 'New task' });
	await dialog.getByLabel('Title').fill('Fix the build');
	await dialog.getByRole('button', { name: 'Create task' }).click();
	await expect(page.getByText('Task T-1 created.')).toBeVisible();

	await page.getByTestId('task-card').filter({ hasText: 'Fix the build' }).getByRole('button', { name: 'Fix the build' }).click();
	const sheet = page.getByRole('dialog');
	await sheet.getByLabel('Description').fill('the build is red');
	await sheet.getByLabel('Description').blur();
	await expect(sheet.getByTestId('saved')).toHaveText('Description saved.');
	await sheet.getByRole('button', { name: 'Draft or improve Description' }).click();
	const panel = sheet.getByTestId('copilot-task-description');
	await panel.getByRole('button', { name: 'Improve' }).click();
	await panel.getByRole('button', { name: 'Use this draft' }).click();
	await expect(sheet.getByLabel('Description')).toHaveValue('The build is red.');
	await expect(sheet.getByTestId('saved')).toHaveText('Description saved.');

	await sheet.getByRole('button', { name: 'Undo', exact: true }).click();
	await expect(sheet.getByLabel('Description')).toHaveValue('the build is red');
	// Closing and reopening shows what the office kept.
	await page.keyboard.press('Escape');
	await page.getByTestId('task-card').filter({ hasText: 'Fix the build' }).getByRole('button', { name: 'Fix the build' }).click();
	await expect(page.getByRole('dialog').getByLabel('Description')).toHaveValue('the build is red');
});

test('task dialog: without a runtime the description is a plain field', async ({ page }) => {
	await openProject(page, 'Tasks', 'off');
	await page.getByRole('button', { name: 'New task' }).click();
	const dialog = page.getByRole('dialog', { name: 'New task' });
	await expect(dialog.getByLabel('Description')).toBeVisible();
	await expect(smartButtons(dialog)).toHaveCount(0);
});
